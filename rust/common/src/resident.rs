//! O processo residente: arquivo de PID, sinal e trava, como em `waybar_common.py`. Os arquivos
//! são os mesmos do Python (`$XDG_RUNTIME_DIR/<nome>.pid` e `.lock`), então as duas versões se
//! reconhecem e quem chama não muda.
use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// O UID deste processo, para o `-u` do pgrep e do pkill: sem ele, os dois veem e tentam os processos de
/// todos os usuários (outra sessão aberta, uma sessão de teste num contêiner), e um "ainda roda" de outro
/// usuário passa por nosso.
pub fn own_uid() -> String {
    // SAFETY: o getuid não falha
    unsafe { libc::getuid() }.to_string()
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| "/tmp".into())
}

pub fn pidfile(name: &str) -> PathBuf {
    runtime_dir().join(format!("{name}.pid"))
}

pub fn lockfile(name: &str) -> PathBuf {
    runtime_dir().join(format!("{name}.lock"))
}

/// O PID do arquivo, só se ainda for este programa (o número pode ter sido reutilizado). O nome
/// vem no argv[0], ou no argv[1] quando o interpretador aparece antes.
pub fn live_pid(pidfile: &Path, name: &str) -> Option<i32> {
    let pid: i32 = std::fs::read_to_string(pidfile).ok()?.trim().parse().ok()?;
    if pid <= 0 {
        return None;
    }
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let found = cmdline
        .split(|b| *b == 0)
        .take(2)
        .any(|arg| Path::new(OsStr::from_bytes(arg)).file_name() == Some(OsStr::new(name)));
    found.then_some(pid)
}

/// Manda o sinal ao programa do arquivo de PID, se ele ainda estiver lá. Devolve se conseguiu.
pub fn signal_pid(pidfile: &Path, name: &str, signal: i32) -> bool {
    match live_pid(pidfile, name) {
        // Um arquivo velho cujo PID voltou para este mesmo programa: o "outro" é este processo
        Some(pid) if pid as u32 == std::process::id() => false,
        // SAFETY: kill só recebe números; um PID que já saiu dá erro, e não comportamento indefinido
        Some(pid) => unsafe { libc::kill(pid, signal) == 0 },
        None => false,
    }
}

/// A trava de um só residente, ou None. Com `wait`, espera um pouco: o residente antigo de um
/// reinício (pkill e start) pode ainda estar saindo. A trava vale enquanto o arquivo ficar aberto.
pub fn take_lock(path: &Path, wait: Duration) -> Option<File> {
    let file = OpenOptions::new().write(true).create(true).truncate(true).open(path).ok()?;
    let deadline = Instant::now() + wait;
    loop {
        // SAFETY: o descritor é do arquivo aberto acima, que vive até o fim da função ou do retorno
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Some(file);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// O código de saída de um residente que achou outro com a trava: a unit do systemd (packaging/files/units)
/// não o reinicia (RestartPreventExitStatus). Os laços do config do Sway só param no 3 e seguem tentando
pub const EXIT_ANOTHER_INSTANCE: u8 = 4;

/// Avisa no stderr (o journal da unit) que outro residente já roda, com o PID dele se o arquivo de PID
/// ainda for de um processo com este comm, e devolve o EXIT_ANOTHER_INSTANCE.
pub fn another_instance(name: &str, comm: &str, pidfile: Option<&Path>) -> std::process::ExitCode {
    match pidfile.and_then(|file| pid_with_comm(file, comm)) {
        Some(pid) => eprintln!("{name}: another instance is running (pid {pid})"),
        None => eprintln!("{name}: another instance is running"),
    }
    std::process::ExitCode::from(EXIT_ANOTHER_INSTANCE)
}

/// O PID do arquivo, se o processo ainda tiver o comm dado (o set_process_name dos residentes).
fn pid_with_comm(pidfile: &Path, comm: &str) -> Option<i32> {
    let pid: i32 = std::fs::read_to_string(pidfile).ok()?.trim().parse().ok()?;
    let found = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    (pid > 0 && pid as u32 != std::process::id() && found.trim_end() == comm).then_some(pid)
}

/// O nome do processo no /proc/<pid>/comm, que é o que o `pgrep -x` e o `pkill -x` comparam (o
/// argv[0] não muda o comm). Chamar na linha principal: o comm do processo é o dela. Até 15 bytes.
pub fn set_process_name(name: &str) -> bool {
    let Ok(name) = std::ffi::CString::new(name) else { return false };
    // SAFETY: o PR_SET_NAME lê uma string C terminada em zero, que vive até o fim da chamada
    unsafe { libc::prctl(libc::PR_SET_NAME, name.as_ptr() as libc::c_ulong, 0, 0, 0) == 0 }
}

/// Se o argv[0] já é o nome do papel (`program`): só o nome do arquivo conta, o caminho não.
pub fn runs_as(argv0: &OsStr, program: &str) -> bool {
    Path::new(argv0).file_name() == Some(OsStr::new(program))
}

/// Roda de novo com o nome do papel no argv[0], se ainda não estiver com ele: quem confere o arquivo de PID
/// lê o /proc/<pid>/cmdline (o live_pid, o sway-osd, o waybar_common.py), e um componente aberto como
/// `swaydesk-<x>` (pelo PATH, pelo paths::component) não seria achado. O comm (set_process_name) não basta:
/// ele não muda o cmdline. Só volta se não conseguir, com um aviso.
pub fn run_as(program: &str) {
    use std::os::unix::process::CommandExt;
    let argv0 = std::env::args_os().next().unwrap_or_default();
    if runs_as(&argv0, program) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let err = std::process::Command::new(exe).arg0(program).args(std::env::args_os().skip(1)).exec();
    eprintln!("{program}: could not run as {program}: {err}");
}

/// Grava o PID deste processo.
pub fn write_pid(pidfile: &Path) -> std::io::Result<()> {
    std::fs::write(pidfile, std::process::id().to_string())
}

/// Apaga o arquivo de PID, só se ainda for o deste processo.
pub fn remove_own_pid(pidfile: &Path, name: &str) {
    if live_pid(pidfile, name) == Some(std::process::id() as i32) {
        let _ = std::fs::remove_file(pidfile);
    }
}

/// Avisa (SIGUSR1) os ícones de um módulo da Waybar, um processo por barra, de que algo mudou: o
/// waybar-notify em Rust, para os popups não dependerem do script. Os arquivos são os mesmos
/// (`$XDG_RUNTIME_DIR/<nome>.<pid>.pid`, gravados pelos ícones em bash, Python e Rust). Um arquivo que não
/// aponta mais para o ícone (PID morto ou reutilizado) é apagado, como no script. A diferença: o nome vale
/// também no argv[0] (o live_pid), e não só no argv[1], então o ícone em Rust (waybar-system) também recebe.
/// É só um aviso: nunca falha. Devolve quantos foram avisados.
pub fn notify_icons(name: &str) -> usize {
    notify_icons_in(&runtime_dir(), name)
}

pub fn notify_icons_in(dir: &Path, name: &str) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let prefix = format!("{name}.");
    let mut notified = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        // O "<nome>.*.pid" do script: o nome, um ponto, algo e .pid (o <nome>.pid do residente não entra)
        let Some(middle) = file_name.strip_prefix(&prefix).and_then(|rest| rest.strip_suffix(".pid")) else { continue };
        if middle.is_empty() {
            continue;
        }
        let path = entry.path();
        // Só some o arquivo que não aponta mais para o ícone; um kill que falha (EPERM) num ícone vivo deixa o
        // arquivo, como no script
        match live_pid(&path, name) {
            None => {
                let _ = std::fs::remove_file(&path);
            }
            Some(_) if signal_pid(&path, name, libc::SIGUSR1) => notified += 1,
            Some(_) => {}
        }
    }
    notified
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swaydesk-resident-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn own_name() -> String {
        std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn runs_as_compares_only_the_file_name() {
        assert!(runs_as(OsStr::new("sway-osd-bar"), "sway-osd-bar"));
        assert!(runs_as(OsStr::new("/usr/libexec/swaydesk/sway-osd-bar"), "sway-osd-bar"));
        // O nome do pacote no PATH ou no /usr/bin precisa do reexec: o sway-osd confere o cmdline
        assert!(!runs_as(OsStr::new("swaydesk-osd-bar"), "sway-osd-bar"));
        assert!(!runs_as(OsStr::new("/usr/bin/swaydesk-osd-bar"), "sway-osd-bar"));
        assert!(!runs_as(OsStr::new(""), "sway-osd-bar"));
    }

    #[test]
    fn live_pid_checks_the_program_name() {
        let dir = tmp("pid");
        let file = dir.join("x.pid");
        write_pid(&file).unwrap();
        assert_eq!(live_pid(&file, &own_name()), Some(std::process::id() as i32));
        assert_eq!(live_pid(&file, "another-program"), None);
        std::fs::write(&file, "garbage").unwrap();
        assert_eq!(live_pid(&file, &own_name()), None);
        assert_eq!(live_pid(&dir.join("missing.pid"), &own_name()), None);
        assert!(!signal_pid(&dir.join("missing.pid"), &own_name(), 0));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Os casos do waybar-notify: avisa o ícone vivo (pelo argv[0] e pelo argv[1], "bash <caminho>/<nome>"),
    /// apaga o arquivo de PID morto, de lixo ou de outro programa, e não toca no <nome>.pid nem em outro nome.
    #[test]
    fn notify_icons_signals_live_icons_and_drops_stale_files() {
        use std::os::unix::process::{CommandExt, ExitStatusExt};
        let dir = tmp("notify");
        let name = "waybar-media";
        // O sleep morre com o SIGUSR1 (ação padrão), o que prova que o aviso chegou
        let mut by_argv0 = std::process::Command::new("sleep").arg0(name).arg("30").spawn().unwrap();
        let script = dir.join(name);
        std::fs::write(&script, "sleep 30\n").unwrap();
        let mut by_argv1 = std::process::Command::new("sh").arg(&script).spawn().unwrap();
        let mut other = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        // O sh do segundo precisa ter lido o script; o cmdline já vale desde o exec
        std::thread::sleep(Duration::from_millis(200));
        let write = |file: &str, text: String| std::fs::write(dir.join(file), text).unwrap();
        write(&format!("{name}.1.pid"), by_argv0.id().to_string());
        write(&format!("{name}.2.pid"), by_argv1.id().to_string());
        write(&format!("{name}.3.pid"), other.id().to_string());
        write(&format!("{name}.4.pid"), "garbage".into());
        write(&format!("{name}.5.pid"), "2147483000".into());
        write(&format!("{name}.pid"), by_argv0.id().to_string()); // o do residente, fora do "<nome>.*.pid"
        write("waybar-volume.6.pid", by_argv0.id().to_string()); // outro módulo
        assert_eq!(notify_icons_in(&dir, name), 2);
        assert_eq!(by_argv0.wait().unwrap().signal(), Some(libc::SIGUSR1));
        assert_eq!(by_argv1.wait().unwrap().signal(), Some(libc::SIGUSR1));
        let left: std::collections::BTreeSet<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        // Os vivos ficam; os de PID morto, lixo e outro programa somem
        let expected = [name.to_string(), format!("{name}.1.pid"), format!("{name}.2.pid"), format!("{name}.pid"), "waybar-volume.6.pid".into()];
        assert_eq!(left, expected.into_iter().collect());
        let _ = other.kill();
        let _ = other.wait();
        assert_eq!(notify_icons_in(&dir.join("missing"), name), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// O mesmo resultado do script home/.local/bin/waybar-notify nos casos dele (ícone em bash ou Python,
    /// "interpretador <caminho>/<nome>"): rodam os dois sobre pastas iguais, e sobram os mesmos arquivos e o
    /// mesmo ícone avisado. O argv[0] só o Rust aceita (o teste acima).
    #[test]
    fn notify_icons_matches_the_waybar_notify_script() {
        use std::os::unix::process::ExitStatusExt;
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../home/.local/bin/waybar-notify");
        assert!(script.is_file(), "{}", script.display());
        let name = "waybar-brightness";
        let run = |dir: &Path, rust: bool| -> (std::collections::BTreeSet<String>, Option<i32>) {
            let icon_script = dir.join(name);
            std::fs::write(&icon_script, "sleep 30\n").unwrap();
            let mut icon = std::process::Command::new("sh").arg(&icon_script).spawn().unwrap();
            let mut other = std::process::Command::new("sleep").arg("30").spawn().unwrap();
            std::thread::sleep(Duration::from_millis(200));
            let write = |file: &str, text: String| std::fs::write(dir.join(file), text).unwrap();
            write(&format!("{name}.1.pid"), icon.id().to_string());
            write(&format!("{name}.2.pid"), other.id().to_string());
            write(&format!("{name}.3.pid"), "garbage".into());
            write(&format!("{name}.4.pid"), "2147483000".into());
            write("waybar-volume.5.pid", icon.id().to_string());
            if rust {
                notify_icons_in(dir, name);
            } else {
                let status = std::process::Command::new("bash").arg(&script).arg(name).env("XDG_RUNTIME_DIR", dir).status().unwrap();
                assert!(status.success());
            }
            let signal = icon.wait().unwrap().signal();
            let _ = other.kill();
            let _ = other.wait();
            let left = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|f| f.ends_with(".pid"))
                .collect();
            (left, signal)
        };
        let (bash_dir, rust_dir) = (tmp("notify-bash"), tmp("notify-rust"));
        let bash = run(&bash_dir, false);
        let rust = run(&rust_dir, true);
        assert_eq!(bash.1, Some(libc::SIGUSR1));
        assert_eq!(rust, bash);
        std::fs::remove_dir_all(bash_dir).unwrap();
        std::fs::remove_dir_all(rust_dir).unwrap();
    }

    #[test]
    fn another_instance_names_the_pid_only_when_the_comm_matches() {
        let dir = tmp("another");
        let file = dir.join("x.pid");
        std::fs::write(&file, "1").unwrap(); // o init: outro comm
        assert_eq!(pid_with_comm(&file, "sway-test-none"), None);
        let comm = std::fs::read_to_string("/proc/1/comm").map(|c| c.trim_end().to_owned()).unwrap_or_default();
        if !comm.is_empty() {
            assert_eq!(pid_with_comm(&file, &comm), Some(1));
        }
        write_pid(&file).unwrap(); // este processo: nunca é o "outro"
        let own = std::fs::read_to_string("/proc/self/comm").unwrap();
        assert_eq!(pid_with_comm(&file, own.trim_end()), None);
        assert_eq!(pid_with_comm(&dir.join("missing.pid"), "x"), None);
        assert_eq!(another_instance("swaydesk-test", "x", None), std::process::ExitCode::from(EXIT_ANOTHER_INSTANCE));
        assert_eq!(EXIT_ANOTHER_INSTANCE, 4);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn process_name_is_what_pgrep_sees() {
        // Cada teste roda numa linha própria: o nome muda só o comm dela
        assert!(set_process_name("sway-drawer"));
        assert_eq!(std::fs::read_to_string("/proc/thread-self/comm").unwrap().trim(), "sway-drawer");
    }

    #[test]
    fn remove_own_pid_leaves_another_programs_file() {
        let dir = tmp("remove");
        let file = dir.join("x.pid");
        std::fs::write(&file, "1").unwrap(); // o init, que não é este programa
        remove_own_pid(&file, &own_name());
        assert!(file.exists());
        write_pid(&file).unwrap();
        remove_own_pid(&file, &own_name());
        assert!(!file.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lock_is_exclusive_and_waits_for_the_old_holder() {
        let dir = tmp("lock");
        let path = dir.join("x.lock");
        let old = take_lock(&path, Duration::ZERO).unwrap();
        assert!(take_lock(&path, Duration::ZERO).is_none()); // o toggle não espera
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(old);
        });
        assert!(take_lock(&path, Duration::from_secs(3)).is_some());
        releaser.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
