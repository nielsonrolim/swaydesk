//! Ícone "Sistema" na Waybar (custom/system): o porte de `home/.local/bin/waybar-system`, que passa a vez
//! a este binário (sway_rust). Uma CPU, em laranja quando a CPU fica alta, a memória quase cheia ou a CPU
//! quente; a dica mostra os números.
//!
//! Lê o /proc e o /sys a cada 3 s, sem abrir processos, e só escreve quando muda. O popup manda SIGUSR1
//! (waybar-notify), para a dica sumir enquanto ele está aberto e voltar quando fecha.
use std::io::Write;
use std::process::ExitCode;

use swaydesk_common::i18n::{decimal, tr};
use swaydesk_common::{config, i18n, resident};
use swaydesk_system_logic as ss;

const NAME: &str = "waybar-system";
const ICON: &str = "\u{f035b}"; // cpu (memory, Material da Nerd Font)
const INTERVAL_S: i64 = 3;

/// A linha JSON para a Waybar: o texto (a CPU), a dica e a classe.
fn line(cpu_history: &[f64], mem: &ss::Memory, temp: Option<&ss::Temperature>, popup_open: bool) -> String {
    let mut parts = vec![
        match cpu_history.last() {
            Some(cpu) => tr("CPU: %s%%").replace("%s", &decimal(*cpu, 0)).replace("%%", "%"),
            None => tr("CPU: …"),
        },
        tr("Memory: %(used)s of %(total)s")
            .replace("%(used)s", &ss::size(mem.used, decimal))
            .replace("%(total)s", &ss::size(mem.total, decimal)),
    ];
    if let Some(temp) = temp {
        parts.push(tr("Temperature: %s °C").replace("%s", &decimal(temp.celsius, 0)));
    }
    let tooltip = if popup_open { String::new() } else { parts.join("\n") };
    let class = if ss::warn_state(cpu_history, mem, temp) { "warn" } else { "normal" };
    serde_json::json!({"text": ICON, "tooltip": tooltip, "class": class}).to_string()
}

/// Os sinais esperados pelo laço, bloqueados (ficam pendentes): o SIGUSR1 do popup e os de encerrar,
/// para o arquivo do PID sair junto.
const WAITED: [i32; 4] = [libc::SIGUSR1, libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

fn waited_set() -> libc::sigset_t {
    // SAFETY: só preenche um sigset_t local
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for signal in WAITED {
            libc::sigaddset(&mut set, signal);
        }
        set
    }
}

/// O que acordou o laço depois de até `seconds`.
enum Wake {
    Timeout,
    Popup,
    Quit,
}

fn wait_signal(seconds: i64) -> Wake {
    let set = waited_set();
    let timeout = libc::timespec { tv_sec: seconds, tv_nsec: 0 };
    loop {
        // SAFETY: o set e o timespec são locais
        match unsafe { libc::sigtimedwait(&set, std::ptr::null_mut(), &timeout) } {
            libc::SIGUSR1 => return Wake::Popup,
            got if got > 0 => return Wake::Quit,
            // Interrompido por um sinal que não é daqui: espera de novo
            _ if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) => continue,
            _ => return Wake::Timeout,
        }
    }
}

/// A máscara passa para os filhos num exec: se o ícone um dia abrir processos, desbloqueie no pre_exec.
fn block_waited() {
    let set = waited_set();
    // SAFETY: muda a máscara do processo antes de qualquer linha
    unsafe { libc::sigprocmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
}

/// A Waybar reiniciada fecha a ponta de leitura do pipe; sem escrever nada, o POLLERR é o único aviso.
fn waybar_gone(parent: i32) -> bool {
    let mut fd = libc::pollfd { fd: 1, events: 0, revents: 0 };
    // SAFETY: poll num fd e getppid, sem ponteiros guardados
    unsafe { libc::poll(&mut fd, 1, 0) > 0 || libc::getppid() != parent }
}

fn run() -> ExitCode {
    // Bloqueados antes do arquivo do PID: a ação padrão do SIGUSR1 encerraria o processo
    block_waited();
    i18n::init(&config::load());
    let pidfile = resident::runtime_dir().join(format!("{NAME}.{}.pid", std::process::id()));
    let _ = std::fs::write(&pidfile, std::process::id().to_string()); // um por barra, como os ícones em bash
    let (proc, sys) = (ss::proc_root(), ss::sys_root());
    // SAFETY: getppid não tem efeito colateral
    let parent = unsafe { libc::getppid() };
    let popup_pidfile = resident::pidfile("waybar-system-popup");
    let mut last: Option<String> = None;
    let mut history: Vec<f64> = Vec::new();
    let mut before: Option<(u64, u64)> = None;
    let mut woken = false;
    let mut stdout = std::io::stdout();
    loop {
        // Acordado pelo popup, só a dica muda: uma medida de CPU de poucos milissegundos seria só ruído
        if !woken {
            if let Ok(after) = ss::cpu_times(&proc) {
                if let Some(b) = before.filter(|b| after.1 > b.1) {
                    history.push(ss::cpu_percent(b, after));
                    let n = history.len();
                    history.drain(..n.saturating_sub(2));
                }
                before = Some(after);
            }
        }
        let popup_open = resident::live_pid(&popup_pidfile, "waybar-system-popup").is_some();
        let current = match ss::memory(&proc) {
            Ok(mem) => Some(line(&history, &mem, ss::temperature(&sys).as_ref(), popup_open)),
            Err(err) => {
                // Um /proc ilegível não derruba o ícone; tenta de novo depois
                eprintln!("{NAME}: {err}");
                last.clone()
            }
        };
        if let Some(current) = current.filter(|c| Some(c) != last.as_ref()) {
            if writeln!(stdout, "{current}").and_then(|_| stdout.flush()).is_err() {
                break; // a Waybar foi embora
            }
            last = Some(current);
        }
        woken = match wait_signal(INTERVAL_S) {
            Wake::Popup => true,
            Wake::Timeout => false,
            Wake::Quit => break,
        };
        if waybar_gone(parent) {
            break;
        }
    }
    let _ = std::fs::remove_file(&pidfile);
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-system em Python confere que o binário carrega
    }
    run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_like_python() {
        let mem = ss::Memory { total: 8 << 30, used: 6 << 30, swap_total: 0, swap_used: 0 };
        let hot = ss::Temperature { celsius: 95.4, critical: None };
        let v: serde_json::Value = serde_json::from_str(&line(&[10.0, 12.4], &mem, Some(&hot), false)).unwrap();
        assert_eq!(v["text"], ICON);
        assert_eq!(v["class"], "warn"); // quente
        // Na língua da config de quem roda o teste: só a forma e os números
        let tip: Vec<&str> = v["tooltip"].as_str().unwrap().lines().collect();
        assert_eq!(tip.len(), 3);
        assert!(tip[0].ends_with("12%"), "{tip:?}");
        assert!(tip[1].contains(&ss::size(6 << 30, decimal)) && tip[1].contains(&ss::size(8 << 30, decimal)), "{tip:?}");
        assert!(tip[2].ends_with("95 °C"), "{tip:?}");
        let v: serde_json::Value = serde_json::from_str(&line(&[], &mem, None, true)).unwrap();
        assert_eq!((v["class"].as_str(), v["tooltip"].as_str()), (Some("normal"), Some("")));
        let v: serde_json::Value = serde_json::from_str(&line(&[], &mem, None, false)).unwrap();
        assert_eq!(v["tooltip"].as_str().unwrap().lines().count(), 2);
    }
}
