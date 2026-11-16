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
}
