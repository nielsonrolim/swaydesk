//! O swaydesk-lock sozinho (sem argumentos) bloqueia pelo sway-lock do pacote: troca de processo para ele e
//! sai com o código dele. O binário de verdade, copiado para uma árvore de teste com um sway-lock falso no
//! lugar do pacote (bin/../libexec/swaydesk) ou ao lado (os dotfiles). Nada aqui abre uma tela: o exec vem
//! antes do GTK. O "já estava bloqueado" (o F do protocolo, volta 0 sem um segundo locker) é do próprio
//! sway-lock: tests/test_sway_lock.py, test_already_locked.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// Os testes rodam em threads do mesmo processo. Um spawn de uma thread herda, entre o fork e o exec, o fd de
/// escrita que outra thread ainda tem aberto num executável novo (o swaydesk-lock copiado, um sway-lock falso); se
/// essa outra thread executa o arquivo nesse intervalo, o exec falha com ETXTBSY ("Text file busy"). Esta trava
/// separa as duas coisas: a escrita de um executável e o spawn (que só volta depois do exec do filho, quando os fds
/// herdados com CLOEXEC já fecharam).
static EXEC: Mutex<()> = Mutex::new(());

/// A árvore de um teste, apagada quando ele termina (passando ou não): a cópia do binário de debug tem dezenas de MB,
/// e cada execução deixava a sua no CARGO_TARGET_TMPDIR.
struct Tree(PathBuf);

impl std::ops::Deref for Tree {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tree(name: &str) -> Tree {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("lock-no-args-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).unwrap();
    let _exec = EXEC.lock().unwrap_or_else(|e| e.into_inner());
    fs::copy(env!("CARGO_BIN_EXE_swaydesk-lock"), root.join("bin/swaydesk-lock")).unwrap();
    Tree(root)
}

/// Um sway-lock falso: grava os argumentos (uma linha) e sai com o código pedido.
fn fake_sway_lock(path: &Path, code: i32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let log = path.with_file_name("sway-lock.args");
    let _exec = EXEC.lock().unwrap_or_else(|e| e.into_inner());
    fs::write(path, format!("#!/bin/sh\necho \"[$*]\" >> '{}'\nexit {code}\n", log.display())).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    let child = {
        let _exec = EXEC.lock().unwrap_or_else(|e| e.into_inner());
        Command::new(root.join("bin/swaydesk-lock"))
            .args(args)
            .env("HOME", root.join("home"))
            .env("PATH", "/usr/bin:/bin")
            .env_remove("SWAYDESK_SESSION")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    child.wait_with_output().unwrap()
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
