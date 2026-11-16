//! A conversa com o Sway pelo swaymsg: ler as saídas, aplicar comandos e assinar os eventos.
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdout, Command, Stdio};

use serde_json::Value;
use swaydesk_displays_logic::outputs::{self, Output};

/// O ambiente do filho: sem sinais bloqueados (veja o swaydesk_ui::cmd::child_env) e sem o GSK_RENDERER
/// que este processo definiu
pub fn child_env(command: &mut Command) -> &mut Command {
    // SAFETY: entre o fork e o exec só roda o sigprocmask, que é seguro nesse intervalo
    unsafe {
        command.pre_exec(|| {
            let mut empty: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut empty);
            libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
            Ok(())
        });
    }
    swaydesk_common::renderer::clean_command(command)
}

/// As saídas, ou None se o Sway não respondeu em 3 s
pub fn get_outputs() -> Option<Vec<Output>> {
    let out = child_env(&mut Command::new("timeout"))
        .args(["-k", "1", "3", "swaymsg", "-r", "-t", "get_outputs"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(outputs::parse(&value))
}

/// Aplica os comandos (já montados pelo `config::commands`). Err com a mensagem do Sway, se algum falhou.
pub fn run(commands: &str) -> Result<(), String> {
    if commands.is_empty() {
        return Ok(());
    }
    // Trocar o modo de várias telas pode levar alguns segundos (o monitor ressincroniza)
    let out = child_env(&mut Command::new("timeout"))
        .args(["-k", "1", "15", "swaymsg", "-r", "--", commands])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|err| format!("swaymsg: {err}"))?;
    let replies: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    let errors: Vec<String> = replies
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r.get("success").and_then(Value::as_bool) == Some(false))
        .map(|r| r.get("error").and_then(Value::as_str).unwrap_or("?").to_string())
        .collect();
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    if !out.status.success() {
        return Err(format!("swaymsg: exit {}", out.status.code().unwrap_or(-1)));
    }
    Ok(())
}

/// As saídas que não ficaram como pedido (ligada ou desligada). Uma saída que sumiu não conta.
fn off_target(wanted: &[(String, bool)]) -> Option<Vec<String>> {
    let now = get_outputs()?;
    Some(
        wanted
            .iter()
            .filter(|(name, on)| now.iter().any(|o| &o.name == name && o.active != *on))
            .map(|(name, _)| name.clone())
            .collect(),
    )
}

/// Aplica e confere que cada saída ficou ligada ou desligada como pedido. O Sway responde "ok" mesmo
/// quando o backend recusa ligar a tela (o "Backend commit failed" do sway.log, visto numa VM logo depois
/// de um disable): os comandos vão de novo depois de uma pausa. Err se ainda não ficou.
pub fn run_checked(commands: &str, wanted: &[(String, bool)]) -> Result<(), String> {
    const PAUSES: [u64; 3] = [300, 1000, 2000];
    run(commands)?;
    for (attempt, pause) in PAUSES.iter().enumerate() {
        std::thread::sleep(std::time::Duration::from_millis(*pause));
        let missing = match off_target(wanted) {
            None => return Ok(()), // sem resposta do Sway: fica o "ok" do comando
            Some(missing) if missing.is_empty() => return Ok(()),
            Some(missing) => missing,
        };
        if attempt + 1 == PAUSES.len() {
            return Err(format!("{}: {}", "the screen did not switch", missing.join(", ")));
        }
        run(commands)?;
    }
    Ok(())
}

/// As áreas de trabalho: (nome, saída, com o foco)
fn workspaces() -> Option<Vec<(String, String, bool)>> {
    let out = child_env(&mut Command::new("timeout"))
        .args(["-k", "1", "3", "swaymsg", "-r", "-t", "get_workspaces"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let value: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|w| {
                let name = w.get("name")?.as_str()?.to_string();
                let output = w.get("output")?.as_str()?.to_string();
                Some((name, output, w.get("focused").and_then(Value::as_bool).unwrap_or(false)))
            })
            .collect(),
    )
}

/// A área de trabalho com o foco e a saída dela (onde a janela do app está), para voltar depois
pub fn focused_workspace() -> Option<(String, String)> {
    workspaces()?.into_iter().find(|(_, _, focused)| *focused).map(|(name, output, _)| (name, output))
}

/// O comando que leva a área de trabalho de volta à saída, ou None se não precisa (ou se o nome não cabe
/// num comando entre aspas)
pub fn return_command(name: &str, home: &str, now: &[(String, String, bool)]) -> Option<String> {
    // Os mesmos caracteres que o quotable() do conf.rs recusa: as aspas, o escape, os separadores de
    // comando e o $ (o Sway troca as variáveis nos comandos)
    let quotable = !name.contains(['"', '\\', ';', ',', '$']) && !name.chars().any(char::is_control);
    if !swaydesk_displays_logic::config::safe_name(home) || !quotable {
        return None;
    }
    let (_, output, _) = now.iter().find(|(n, _, _)| n == name)?;
    (output != home).then(|| format!("workspace \"{name}\"; move workspace to output {home}"))
}

/// Desligar uma tela leva as áreas de trabalho dela para outra, e religar não as traz de volta: a do app
/// (e a janela, que a pessoa estava usando) volta à tela de onde saiu
pub fn return_workspace(name: &str, home: &str) {
    let Some(now) = workspaces() else { return };
    let home_on = get_outputs().is_some_and(|list| list.iter().any(|o| o.name == home && o.active));
    if let Some(command) = return_command(name, home, &now).filter(|_| home_on) {
        let _ = run(&command);
    }
}

/// O tipo de evento que interessa
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Uma saída entrou, saiu ou mudou
    Output,
    /// O config foi relido (swaymsg reload): o 90-displays.conf pode ter trocado o layout
    Reload,
}

/// O evento de uma linha do `swaymsg -t subscribe -m '["output","workspace"]'`. O de saída é só
/// {"change": "unspecified"}; o de workspace sempre traz "current", e só o "reload" interessa.
pub fn event(line: &str) -> Option<Event> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    let change = value.get("change").and_then(Value::as_str)?;
    if change == "reload" {
        Some(Event::Reload)
    } else if value.get("current").is_none() && value.get("old").is_none() {
        Some(Event::Output)
    } else {
        None
    }
}

/// O swaymsg que assina os eventos. setpriv: ele morre junto com este processo, mesmo num SIGKILL.
pub fn subscribe() -> std::io::Result<(Child, BufReader<ChildStdout>)> {
    let mut child = child_env(&mut Command::new("setpriv"))
        .args(["--pdeathsig", "TERM", "swaymsg", "-t", "subscribe", "-m", r#"["output","workspace"]"#])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("no stdout"))?;
    Ok((child, BufReader::new(stdout)))
}

/// Lê a próxima linha (em bytes: um nome com UTF-8 inválido não para a leitura). None no fim.
pub fn next_line(reader: &mut BufReader<ChildStdout>) -> Option<String> {
    let mut line = Vec::new();
    match reader.read_until(b'\n', &mut line) {
        Ok(n) if n > 0 => Some(String::from_utf8_lossy(&line).into_owned()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events() {
        assert_eq!(event(r#"{"change": "unspecified"}"#), Some(Event::Output));
        assert_eq!(event(r#"{"change": "reload", "current": null, "old": null}"#), Some(Event::Reload));
        assert_eq!(event(r#"{"change": "focus", "current": {}, "old": {}}"#), None);
        assert_eq!(event(r#"{"success": true}"#), None);
        assert_eq!(event("garbage"), None);
    }

    #[test]
    fn returns_the_workspace() {
        let now = vec![("1".to_string(), "HDMI-A-1".to_string(), true), ("2".to_string(), "HDMI-A-1".to_string(), false)];
        assert_eq!(return_command("1", "eDP-2", &now).as_deref(), Some("workspace \"1\"; move workspace to output eDP-2"));
        assert_eq!(return_command("1", "HDMI-A-1", &now), None); // já está lá
        assert_eq!(return_command("9", "eDP-2", &now), None); // sumiu
        let odd = vec![("a\"b".to_string(), "HDMI-A-1".to_string(), true)];
        assert_eq!(return_command("a\"b", "eDP-2", &odd), None);
        assert_eq!(return_command("1", "eDP 2", &now), None);
        // O Sway troca as variáveis ($mod) nos comandos: o alvo seria outro
        let var = vec![("$mod".to_string(), "HDMI-A-1".to_string(), true)];
        assert_eq!(return_command("$mod", "eDP-2", &var), None);
    }
}
