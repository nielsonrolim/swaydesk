//! Com outro swaydesk-switcher segurando a trava, este sai com o 4 (swaydesk_common::resident::EXIT_ANOTHER_INSTANCE), que
//! a unit do systemd não reinicia (RestartPreventExitStatus em packaging/files/units), e diz no stderr por quê.
//! A trava é tomada antes do GTK: o teste não precisa de tela.
use std::os::fd::AsRawFd;
use std::process::Command;

#[test]
fn a_second_copy_exits_with_4_and_says_why() {
    let dir = std::env::temp_dir().join(format!("swaydesk-another-switcher-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lock = std::fs::File::create(dir.join("sway-switcher.lock")).unwrap();
    // SAFETY: o descritor é do arquivo aberto acima, que vive até o fim do teste
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    let out = Command::new(env!("CARGO_BIN_EXE_swaydesk-switcher"))
        .args(["daemon"])
        .env("XDG_RUNTIME_DIR", &dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("SWAYSOCK")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(4), "{stderr}");
    assert!(stderr.contains("swaydesk-switcher: another instance is running"), "{stderr}");
    drop(lock);
    std::fs::remove_dir_all(dir).unwrap();
}
