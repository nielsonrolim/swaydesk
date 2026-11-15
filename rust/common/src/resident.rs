//! O processo residente: arquivo de PID, sinal e trava, como em `waybar_common.py`. Os arquivos
//! são os mesmos do Python (`$XDG_RUNTIME_DIR/<nome>.pid` e `.lock`), então as duas versões se
//! reconhecem e quem chama não muda.
use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// O nome do processo no /proc/<pid>/comm, que é o que o `pgrep -x` e o `pkill -x` comparam (o
/// argv[0] não muda o comm). Chamar na linha principal: o comm do processo é o dela. Até 15 bytes.
pub fn set_process_name(name: &str) -> bool {
    let Ok(name) = std::ffi::CString::new(name) else { return false };
    // SAFETY: o PR_SET_NAME lê uma string C terminada em zero, que vive até o fim da chamada
    unsafe { libc::prctl(libc::PR_SET_NAME, name.as_ptr() as libc::c_ulong, 0, 0, 0) == 0 }
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
