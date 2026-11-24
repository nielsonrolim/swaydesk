//! O swaydesk-lock sozinho (sem argumentos) bloqueia pelo sway-lock do pacote: troca de processo para ele e
//! sai com o código dele. O binário de verdade, copiado para uma árvore de teste com um sway-lock falso no
//! lugar do pacote (bin/../libexec/swaydesk) ou ao lado (os dotfiles). Nada aqui abre uma tela: o exec vem
//! antes do GTK. O "já estava bloqueado" (o F do protocolo, volta 0 sem um segundo locker) é do próprio
//! sway-lock: tests/test_sway_lock.py, test_already_locked.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn tree(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("lock-no-args-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_swaydesk-lock"), root.join("bin/swaydesk-lock")).unwrap();
    root
}

/// Um sway-lock falso: grava os argumentos (uma linha) e sai com o código pedido.
fn fake_sway_lock(path: &Path, code: i32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let log = path.with_file_name("sway-lock.args");
    fs::write(path, format!("#!/bin/sh\necho \"[$*]\" >> '{}'\nexit {code}\n", log.display())).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(root.join("bin/swaydesk-lock"))
        .args(args)
        .env("HOME", root.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .env_remove("SWAYDESK_SESSION")
        .output()
        .unwrap()
}

#[test]
fn no_args_execs_the_packages_sway_lock_and_keeps_its_code() {
    let root = tree("package");
    let script = root.join("libexec/swaydesk/sway-lock");
    fake_sway_lock(&script, 0);
    let out = run(&root, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    // Sem argumentos para o sway-lock: ele é quem chama o swaydesk-lock --notify-fd N
    assert_eq!(fs::read_to_string(script.with_file_name("sway-lock.args")).unwrap(), "[]\n");
    // Nada bloqueou: o código do sway-lock chega a quem chamou (o swayidle, o atalho)
    fake_sway_lock(&script, 1);
    assert_eq!(run(&root, &[]).status.code(), Some(1));
}

#[test]
fn no_args_in_the_dotfiles_uses_the_sway_lock_beside_it() {
    let root = tree("dotfiles");
    let script = root.join("bin/sway-lock");
    fake_sway_lock(&script, 0);
    assert_eq!(run(&root, &[]).status.code(), Some(0));
    assert_eq!(fs::read_to_string(script.with_file_name("sway-lock.args")).unwrap(), "[]\n");
}

#[test]
fn no_args_never_takes_a_sway_lock_from_the_home_or_the_path() {
    let root = tree("missing");
    fake_sway_lock(&root.join("home/.local/bin/sway-lock"), 0);
    let out = run(&root, &[]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("helper script sway-lock not found"), "{err}");
    assert!(err.contains(&root.join("libexec/swaydesk/sway-lock").display().to_string()), "{err}");
    assert!(err.contains("install the swaydesk-"), "{err}");
    assert!(!root.join("home/.local/bin/sway-lock.args").exists());
}

#[test]
fn help_prints_the_usage_and_a_bad_argument_is_a_usage_error() {
    let root = tree("usage");
    fake_sway_lock(&root.join("libexec/swaydesk/sway-lock"), 0);
    let help = run(&root, &["--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("lock the screen"));
    for bad in [&["--frobnicate"][..], &["--notify-fd"][..], &["--notify-fd", "x"][..], &["extra", "args"][..]] {
        let out = run(&root, bad);
        assert_eq!(out.status.code(), Some(2), "{bad:?}");
        assert!(String::from_utf8_lossy(&out.stderr).starts_with("usage: swaydesk-lock"), "{bad:?}");
    }
    // Nenhum desses chamou o sway-lock
    assert!(!root.join("libexec/swaydesk/sway-lock.args").exists());
}
