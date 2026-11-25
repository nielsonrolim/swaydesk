//! Os programas do projeto que os popups chamam: o `BIN`, o `spawn` e as leituras de status do
//! `waybar_popup.py` e de cada popup.
use std::ffi::OsStr;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

/// Se foi este processo que definiu o GSK_RENDERER (pelo `use_cairo_renderer`): quem abre apps por outro
/// caminho, como o `AppLaunchContext` do Gio, tira a variável só nesse caso.
pub use swaydesk_common::renderer::{clean_launch_context, renderer_set_here};

/// O ambiente de um programa aberto pelo popup: sem o GSK_RENDERER que o popup definiu para si. Um app
/// GTK 4 (o pavucontrol, o Resources) desenharia por CPU, com gráficos e animações mais lentos. Se a
/// variável veio do ambiente, ela passa adiante como antes.
///
/// Também desbloqueia os sinais no filho. O popup bloqueia o SIGTERM e o SIGINT na subida, até o GLib os
/// tratar, e as linhas criadas nesse intervalo (as do GIO, do spawn_blocking) ficam com a máscara: um filho
/// aberto por elas herdava o TERM bloqueado e não morria nem com o kill nem com o --pdeathsig (o swaymsg
/// dos eventos ficava órfão a cada popup, SigBlk 0x4002).
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

/// Um script auxiliar: o de ~/.local/bin, onde o install.sh liga os scripts, ou o da pasta dos scripts do
/// pacote (swaydesk_common::paths::helper).
pub fn bin(name: &str) -> PathBuf {
    swaydesk_common::paths::helper(name)
}

/// A saída de um programa, sem os espaços das pontas, com prazo de 2 s: o `timeout` manda TERM e,
/// 1 s depois, KILL (como o timeout do subprocess do Python), senão quem ignora o TERM prenderia a
/// leitura. Vazia se ele não abriu: os popups tratam como "desligado". Chame fora da linha principal.
pub fn status<S: AsRef<OsStr>>(program: impl AsRef<OsStr>, args: &[S]) -> String {
    child_env(&mut Command::new("timeout"))
        .args(["-k", "1", "2"])
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Um programa solto, numa sessão própria (setsid, como o `start_new_session` do Python): continua
/// depois de o popup fechar. Devolve o filho para
/// quem precisa saber se terminou ([`Child::try_wait`]); quem não guarda usa o [`spawn_detached`].
pub fn spawn<S: AsRef<OsStr>>(program: impl AsRef<OsStr>, args: &[S]) -> std::io::Result<Child> {
    child_env(&mut Command::new(program))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .pre_session()
        .spawn()
}

/// Um app (não um auxiliar): o [`spawn_detached`] e, se este processo roda como serviço do usuário, o app num
/// escopo próprio do systemd (swaydesk_common::app_scope), com o nome do programa como id. Os auxiliares (o
/// swaymsg, os scripts) continuam no [`spawn_detached`], no cgroup do componente.
pub fn spawn_app<S: AsRef<OsStr>>(program: impl AsRef<OsStr>, args: &[S]) -> std::io::Result<()> {
    let id = std::path::Path::new(program.as_ref()).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    use swaydesk_common::app_scope;
    if !app_scope::wants_scope() {
        return spawn_detached(program, args);
    }
    // Num serviço: o app espera no sh do GATE_SCRIPT até a mudança de escopo, e só então vira o app (o mesmo
    // PID). Sem o pipe, abre direto, como antes
    let Ok((read, release)) = app_scope::gate() else { return spawn_detached(program, args) };
    let [sh, flag, script, name] = app_scope::GATE_SCRIPT;
    let mut command = Command::new(sh);
    command.args([flag, script, name]).arg(program).args(args);
    child_env(&mut command).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).pre_session();
    for name in app_scope::UNIT_ENV {
        command.env_remove(name);
    }
    use std::os::fd::AsRawFd;
    let fd = read.as_raw_fd();
    // SAFETY: entre o fork e o exec só roda o dup2 ou o fcntl (gate_fd_in_child)
    unsafe {
        command.pre_exec(move || app_scope::gate_fd_in_child(fd));
    }
    let mut child = command.spawn()?;
    drop(read);
    app_scope::move_then_release(child.id(), &id, release);
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// O [`spawn`] sem guardar o filho: uma linha espera por ele, para não ficar zumbi enquanto o popup
/// está aberto.
pub fn spawn_detached<S: AsRef<OsStr>>(program: impl AsRef<OsStr>, args: &[S]) -> std::io::Result<()> {
    let mut child = spawn(program, args)?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

trait PreSession {
    fn pre_session(&mut self) -> &mut Self;
}

impl PreSession for Command {
    fn pre_session(&mut self) -> &mut Self {
        // SAFETY: entre o fork e o exec só roda o setsid, que é seguro nesse intervalo
        unsafe {
            self.pre_exec(|| {
                libc::setsid();
                Ok(())
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_trims_and_survives_a_missing_program() {
        assert_eq!(status("printf", &["  until 1 12:00\n"]), "until 1 12:00");
        assert_eq!(status("/nonexistent/program", &["status"]), "");
    }

    #[test]
    fn the_renderer_set_by_the_popup_is_not_inherited() {
        // Só este teste mexe no GSK_RENDERER e na marca
        std::env::set_var("GSK_RENDERER", "cairo");
        swaydesk_common::renderer::mark_set_here_for_tests(true);
        assert!(spawn("sh", &["-c", "test -z \"$GSK_RENDERER\""]).unwrap().wait().unwrap().success());
        assert_eq!(status("sh", &["-c", "echo \"[$GSK_RENDERER]\""]), "[]");
        // Vindo do ambiente, passa adiante
        swaydesk_common::renderer::mark_set_here_for_tests(false);
        assert_eq!(status("sh", &["-c", "echo \"[$GSK_RENDERER]\""]), "[cairo]");
        std::env::remove_var("GSK_RENDERER");
    }

    #[test]
    fn children_start_with_no_blocked_signals() {
        // Uma linha com o TERM bloqueado, como as do GIO criadas na subida do popup
        let mask = std::thread::spawn(|| {
            // SAFETY: só a máscara desta linha de teste
            unsafe {
                let mut set: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGTERM);
                libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
            }
            status("grep", &["SigBlk", "/proc/self/status"])
        })
        .join()
        .unwrap();
        assert_eq!(mask.split_whitespace().last(), Some("0000000000000000"), "{mask}");
    }

    #[test]
    fn spawn_runs_in_its_own_session() {
        // Líder da sessão: o sid é o próprio PID
        let mut child = spawn("sh", &["-c", "test \"$(ps -o sid= -p $$)\" -eq $$"]).unwrap();
        assert!(child.wait().unwrap().success());
    }
}
