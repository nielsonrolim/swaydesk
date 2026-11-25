//! Os apps abertos por um componente que roda como serviço do usuário (a dock, a grade: os `swaydesk-*.service`)
//! vão para um escopo próprio do systemd (`app-swaydesk-<id>-<pid>.scope`, na `app.slice`), como fazem o GNOME
//! e o KDE. Sem isso, eles ficavam no cgroup do serviço: apareciam no `systemctl --user status` dele, e um
//! restart com o KillMode padrão os levaria junto.
//!
//! Sem corrida: o processo novo espera num `sh` (o [`GATE_SCRIPT`]) até o `StartTransientUnit` do systemd
//! terminar o job do escopo, e só então vira o app, no mesmo PID. Assim nada que o app criar fica no cgroup do
//! serviço. Um app do Gio (um .desktop) abre pelo auxiliar [`launch`]: o próprio binário do componente, com o
//! [`LAUNCH_ARG`], que já nasce no escopo e chama o Gio de lá. Fora de um serviço (sem o `INVOCATION_ID`), nada
//! muda; se a mudança falhar, o app abre do mesmo jeito, só com uma linha no stderr.
use gio::glib;
use gio::prelude::*;

/// Se este processo roda como serviço do systemd (o systemd define o `INVOCATION_ID` para cada execução).
pub fn under_unit() -> bool {
    std::env::var_os("INVOCATION_ID").is_some_and(|v| !v.is_empty())
}

/// As variáveis que o systemd dá ao serviço e que não são do app: com elas, um terminal aberto pela dock
/// passaria o INVOCATION_ID a cada shell, e um componente aberto à mão ali pensaria rodar num serviço.
pub const UNIT_ENV: [&str; 2] = ["INVOCATION_ID", "JOURNAL_STREAM"];

/// O nome do escopo: o id do .desktop (sem o `.desktop`) escapado como no `systemd-escape`, e o PID.
pub fn scope_name(app_id: &str, pid: u32) -> String {
    let id = app_id.strip_suffix(".desktop").unwrap_or(app_id);
    let mut escaped = String::new();
    for (i, b) in id.bytes().enumerate() {
        let plain = b.is_ascii_alphanumeric() || b == b':' || b == b'_' || (b == b'.' && i > 0);
        if plain {
            escaped.push(b as char);
        } else {
            escaped.push_str(&format!("\\x{b:02x}"));
        }
    }
    if escaped.is_empty() {
        escaped.push_str("app");
    }
    format!("app-swaydesk-{escaped}-{pid}.scope")
}

/// Move o processo para um escopo próprio, se este processo roda como serviço. Assíncrono: a resposta chega
/// pelo laço do GLib, e um erro só vira uma linha no stderr.
pub fn move_to_scope(pid: u32, app_id: &str) {
    move_then(pid, app_id, || {});
}

/// O `sh` que segura o app até a mudança de escopo: ele espera o fim do fd 3 (a ponta de leitura do
/// [`gate`]) e só então troca de programa no mesmo processo (exec). Assim o PID movido já é o do app, e nada
/// que o app criar fica no cgroup do serviço. Sem resposta, ou com erro, a ponta fecha do mesmo jeito: o app
/// abre como antes.
pub const GATE_SCRIPT: [&str; 4] = ["/bin/sh", "-c", r#"read -r _ <&3; exec 3<&-; exec "$@""#, "sh"];

/// As duas pontas de um pipe (com CLOEXEC): a de leitura vai como fd 3 para o [`GATE_SCRIPT`]; a de escrita
/// fica aqui, e o [`move_then_release`] a fecha quando a mudança termina.
pub fn gate() -> std::io::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    use std::os::fd::FromRawFd;
    let mut fds = [0; 2];
    // SAFETY: pipe2 preenche os dois fds, que passam a ser nossos
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: fds válidos, abertos acima e de mais ninguém
    Ok(unsafe { (std::os::fd::OwnedFd::from_raw_fd(fds[0]), std::os::fd::OwnedFd::from_raw_fd(fds[1])) })
}

/// No filho, entre o fork e o exec: a ponta de leitura do [`gate`] como fd 3, sem o CLOEXEC. Se ela já for o
/// fd 3, o dup2 não faz nada (e o CLOEXEC ficaria): aí só o fcntl tira a marca.
///
/// # Safety
/// Só para o intervalo entre o fork e o exec (um `pre_exec`): só chama o dup2 e o fcntl.
pub unsafe fn gate_fd_in_child(fd: std::os::fd::RawFd) -> std::io::Result<()> {
    let ok = if fd == 3 {
        // SAFETY: fcntl num fd válido do filho
        unsafe { libc::fcntl(3, libc::F_SETFD, 0) >= 0 }
    } else {
        // SAFETY: dup2 de um fd válido do filho
        unsafe { libc::dup2(fd, 3) >= 0 }
    };
    if ok { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// Move o processo e, com a resposta (ou o erro), fecha a ponta de escrita do [`gate`]: o app segue.
pub fn move_then_release(pid: u32, app_id: &str, release: std::os::fd::OwnedFd) {
    move_then(pid, app_id, move || drop(release));
}

/// A espera pelo job do escopo, sem o D-Bus: a resposta do `StartTransientUnit` traz o job, e o `JobRemoved`
/// dele diz que o PID já está no escopo. O sinal pode chegar antes da resposta. Cada método diz se o app pode
/// seguir agora (uma vez só).
#[derive(Default)]
pub struct JobWait {
    job: Option<String>,
    removed: Vec<String>,
    released: bool,
}

impl JobWait {
    /// A resposta chegou: com o job (ou sem, numa resposta estranha, que não deve prender o app).
    pub fn on_reply(&mut self, job: Option<&str>) -> bool {
        let done = job.is_none_or(|j| self.removed.iter().any(|r| r == j));
        self.job = job.map(str::to_string);
        done && self.release()
    }
    /// Um job terminou (o `JobRemoved`), o nosso ou o de outro.
    pub fn on_removed(&mut self, job: &str) -> bool {
        if self.job.as_deref() == Some(job) {
            return self.release();
        }
        self.removed.push(job.to_string());
        false
    }
    /// O prazo ou um erro: segue, se ainda não seguiu.
    pub fn give_up(&mut self) -> bool {
        self.release()
    }
    fn release(&mut self) -> bool {
        !std::mem::replace(&mut self.released, true)
    }
}

/// Os argumentos do `sh` do gate para o auxiliar: `<binário> --swaydesk-launch <arquivo> <ação ou ""> [URI...]`.
pub fn launcher_argv(
    exe: &std::path::Path,
    file: &std::path::Path,
    action: Option<&str>,
    uris: &[String],
) -> Vec<std::ffi::OsString> {
    let mut argv = vec![exe.into(), LAUNCH_ARG.into(), file.into(), action.unwrap_or("").into()];
    argv.extend(uris.iter().map(Into::into));
    argv
}

/// Uma entrada `NOME=valor` do ambiente do contexto; sem o `=`, nada.
pub fn env_pair(entry: &std::ffi::OsStr) -> Option<(std::ffi::OsString, std::ffi::OsString)> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = entry.as_bytes();
    let eq = bytes.iter().position(|&b| b == b'=')?;
    Some((std::ffi::OsStr::from_bytes(&bytes[..eq]).into(), std::ffi::OsStr::from_bytes(&bytes[eq + 1..]).into()))
}

/// Uma mudança em andamento: a espera ([`JobWait`]), o que solta o app e a inscrição no `JobRemoved`.
struct Pending {
    wait: JobWait,
    done: Option<Box<dyn FnOnce()>>,
    subscription: Option<gio::SignalSubscriptionId>,
}

/// Solta o app e desfaz a inscrição, quando o [`JobWait`] disse que é a hora (só uma vez).
fn finish(conn: &gio::DBusConnection, pending: &std::rc::Rc<std::cell::RefCell<Pending>>) {
    let (done, subscription) = {
        let mut p = pending.borrow_mut();
        (p.done.take(), p.subscription.take())
    };
    if let Some(subscription) = subscription {
        conn.signal_unsubscribe(subscription);
        // Cada Subscribe com o seu Unsubscribe (o systemd conta os pedidos): sem isso, todo sinal de job e de
        // unit continuaria chegando ao componente pelo resto da vida dele
        conn.call(
            Some("org.freedesktop.systemd1"),
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "Unsubscribe",
            None,
            None,
            gio::DBusCallFlags::NONE,
            2000,
            gio::Cancellable::NONE,
            |_| {},
        );
    }
    if let Some(done) = done {
        done();
    }
}

fn move_then(pid: u32, app_id: &str, done: impl FnOnce() + 'static) {
    use std::cell::RefCell;
    use std::rc::Rc;
    if !under_unit() || pid == 0 {
        return done();
    }
    let name = scope_name(app_id, pid);
    let conn = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("app scope: no session bus: {err}");
            return done();
        }
    };
    let properties: Vec<(String, glib::Variant)> = vec![
        ("PIDs".into(), vec![pid].to_variant()),
        ("Slice".into(), "app.slice".to_variant()),
        ("Description".into(), format!("{app_id} opened by Swaydesk").to_variant()),
        ("CollectMode".into(), "inactive-or-failed".to_variant()),
    ];
    let aux: Vec<(String, Vec<(String, glib::Variant)>)> = Vec::new();
    let params = (name.as_str(), "fail", properties, aux).to_variant();
    let pending = Rc::new(RefCell::new(Pending {
        wait: JobWait::default(),
        done: Some(Box::new(done)),
        subscription: None,
    }));
    // O systemd só manda o JobRemoved a quem pediu (Subscribe); a ordem das mensagens na conexão garante que o
    // pedido chega antes do StartTransientUnit
    let watch = Rc::clone(&pending);
    let id = conn.signal_subscribe(
        Some("org.freedesktop.systemd1"),
        Some("org.freedesktop.systemd1.Manager"),
        Some("JobRemoved"),
        Some("/org/freedesktop/systemd1"),
        None,
        gio::DBusSignalFlags::NONE,
        move |conn, _, _, _, _, params| {
            let Some((_, job, _, _)) = params.get::<(u32, glib::variant::ObjectPath, String, String)>() else {
                return;
            };
            let release = watch.borrow_mut().wait.on_removed(job.as_str());
            if release {
                finish(conn, &watch);
            }
        },
    );
    pending.borrow_mut().subscription = Some(id);
    conn.call(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "Subscribe",
        None,
        None,
        gio::DBusCallFlags::NONE,
        2000,
        gio::Cancellable::NONE,
        |_| {},
    );
    // No máximo 2 s de espera, com ou sem resposta
    let timeout = Rc::clone(&pending);
    let timeout_conn = conn.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
        let release = timeout.borrow_mut().wait.give_up();
        if release {
            finish(&timeout_conn, &timeout);
        }
    });
    let reply_conn = conn.clone();
    conn.call(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "StartTransientUnit",
        Some(&params),
        Some(glib::VariantTy::new("(o)").expect("a valid type")),
        gio::DBusCallFlags::NONE,
        2000,
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(reply) => {
                let job = reply.get::<(glib::variant::ObjectPath,)>().map(|(j,)| j.as_str().to_string());
                let release = pending.borrow_mut().wait.on_reply(job.as_deref());
                if release {
                    finish(&reply_conn, &pending);
                }
            }
            Err(err) => {
                eprintln!("app scope: {name}: {err}");
                let release = pending.borrow_mut().wait.give_up();
                if release {
                    finish(&reply_conn, &pending);
                }
            }
        },
    );
}

/// O argumento que faz o binário da dock ou da grade virar o auxiliar do [`launch`]:
/// `<binário> --swaydesk-launch <arquivo .desktop> <ação ou ""> [URI...]`.
pub const LAUNCH_ARG: &str = "--swaydesk-launch";

/// Abre um app do .desktop (ou uma ação dele, ou com URIs). Num serviço, o auxiliar abre o app de dentro do
/// escopo; fora, ou com um app ativado pelo D-Bus (quem o abre é o barramento, fora do cgroup do serviço), ou se
/// o auxiliar não puder ser criado, o Gio abre direto, como antes.
pub fn launch(
    info: &gio::DesktopAppInfo,
    action: Option<&str>,
    uris: &[String],
    context: Option<&gio::AppLaunchContext>,
) -> Result<(), glib::Error> {
    let direct = || match action {
        Some(action) => {
            info.launch_action(action, context);
            Ok(())
        }
        None => info.launch_uris(&uris.iter().map(String::as_str).collect::<Vec<_>>(), context),
    };
    if !under_unit() || info.boolean("DBusActivatable") {
        return direct();
    }
    match spawn_launcher(info, action, uris, context) {
        Ok(()) => Ok(()),
        Err(err) => {
            eprintln!("app scope: no launcher ({err}); opening it directly");
            direct()
        }
    }
}

fn spawn_launcher(
    info: &gio::DesktopAppInfo,
    action: Option<&str>,
    uris: &[String],
    context: Option<&gio::AppLaunchContext>,
) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::CommandExt;
    let other = |msg: &str| std::io::Error::other(msg.to_string());
    let file = info.filename().ok_or_else(|| other("no .desktop file"))?;
    // O /proc/self/exe não serve: no filho, quem o lê é o sh. Depois de uma atualização, o binário antigo some
    let exe = std::env::current_exe()?;
    if exe.as_os_str().as_bytes().ends_with(b" (deleted)") {
        return Err(other("the binary was replaced"));
    }
    let id = info.id().map(|s| s.to_string()).unwrap_or_else(|| info.name().to_string());
    let (read, release) = gate()?;
    let mut command = std::process::Command::new(GATE_SCRIPT[0]);
    command.args(&GATE_SCRIPT[1..]).args(launcher_argv(&exe, &file, action, uris));
    if let Some(context) = context {
        // O ambiente que o Gio daria ao app (sem o GSK_RENDERER do componente, por exemplo), e o token de
        // ativação do Wayland (o foco da janela nova), que o auxiliar passa adiante
        command.env_clear();
        command.envs(context.environment().iter().filter_map(|entry| env_pair(entry)));
    } else {
        // Sem contexto, o ambiente do componente: sem o GSK_RENDERER que ele escolheu para si
        crate::renderer::clean_command(&mut command);
    }
    if let Some(context) = context {
        if let Some(token) = context.startup_notify_id(Some(info), &[]) {
            command.env("XDG_ACTIVATION_TOKEN", token.as_str()).env("DESKTOP_STARTUP_ID", token.as_str());
        }
    }
    for name in UNIT_ENV {
        command.env_remove(name);
    }
    command.stdin(std::process::Stdio::null());
    let fd = read.as_raw_fd();
    // SAFETY: entre o fork e o exec só rodam o setsid e o dup2/fcntl
    unsafe {
        command.pre_exec(move || {
            libc::setsid();
            gate_fd_in_child(fd)
        });
    }
    let mut child = command.spawn()?;
    drop(read);
    move_then_release(child.id(), &id, release);
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// O lado do auxiliar, no começo do `main` da dock e da grade: com o [`LAUNCH_ARG`], abre o app pelo Gio (já
/// dentro do escopo) e devolve o código de saída; sem ele, `None`.
pub fn launcher_main(args: &[std::ffi::OsString]) -> Option<i32> {
    let [flag, file, action, uris @ ..] = args else { return None };
    if flag != LAUNCH_ARG {
        return None;
    }
    let Some(info) = gio::DesktopAppInfo::from_filename(file) else {
        eprintln!("app scope: cannot read {}", file.to_string_lossy());
        return Some(1);
    };
    let context = gio::AppLaunchContext::new();
    // As URIs são ASCII (o File::uri escapa o resto); o nome da ação vem do .desktop, em UTF-8
    let uris: Vec<String> = uris.iter().map(|u| u.to_string_lossy().into_owned()).collect();
    let action = action.to_string_lossy();
    let result = if action.is_empty() {
        info.launch_uris(&uris.iter().map(String::as_str).collect::<Vec<_>>(), Some(&context))
    } else {
        info.launch_action(&action, Some(&context));
        Ok(())
    };
    match result {
        Ok(()) => Some(0),
        Err(err) => {
            eprintln!("app scope: could not open {}: {}", file.to_string_lossy(), err.message());
            Some(1)
        }
    }
}

/// A rede de segurança dos lançamentos que não passam pelo [`launch`] (o Gio direto, quando o auxiliar não pode
/// ser criado): o app vai para o seu escopo depois de aberto, com o intervalo que o GNOME e o KDE aceitam. Os
/// apps ativados pelo D-Bus (DBusActivatable) não passam por aqui: quem os abre é o barramento.
pub fn scope_launch_context(context: &impl IsA<gio::AppLaunchContext>) {
    if !under_unit() {
        return;
    }
    for name in UNIT_ENV {
        context.unsetenv(name);
    }
    context.connect_launched(|_, info, platform_data| {
        let pid: Option<i32> = glib::VariantDict::new(Some(platform_data)).lookup("pid").ok().flatten();
        if let Some(pid) = pid.filter(|p| *p > 0) {
            let id = info.id().map(|s| s.to_string()).unwrap_or_else(|| info.name().to_string());
            move_to_scope(pid as u32, &id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scope_name_escapes_like_systemd() {
        assert_eq!(scope_name("org.gnome.Nautilus.desktop", 42), "app-swaydesk-org.gnome.Nautilus-42.scope");
        assert_eq!(scope_name("firefox-esr.desktop", 7), "app-swaydesk-firefox\\x2desr-7.scope");
        assert_eq!(scope_name("a b/c", 1), "app-swaydesk-a\\x20b\\x2fc-1.scope");
        assert_eq!(scope_name(".hidden", 1), "app-swaydesk-\\x2ehidden-1.scope");
        assert_eq!(scope_name("", 3), "app-swaydesk-app-3.scope");
    }

    #[test]
    fn the_gate_holds_the_program_until_released() {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let (read, release) = gate().unwrap();
        let fd = read.as_raw_fd();
        let [sh, flag, script, name] = GATE_SCRIPT;
        let mut command = std::process::Command::new(sh);
        command.args([flag, script, name, "echo", "ran"]).stdout(std::process::Stdio::piped());
        // SAFETY: só o dup2 entre o fork e o exec
        unsafe {
            command.pre_exec(move || gate_fd_in_child(fd));
        }
        let mut child = command.spawn().unwrap();
        drop(read);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(child.try_wait().unwrap().is_none(), "ran before the release");
        move_then_release(child.id(), "x.desktop", release); // fora de um serviço: libera na hora
        let out = child.wait_with_output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "ran\n");
    }

    #[test]
    fn the_app_opens_when_the_launcher_goes_away_without_an_answer() {
        // Sem resposta (o componente saiu no meio): a ponta de escrita fecha com ele, o read vê o fim e o app
        // abre do mesmo jeito; o fd 3 não chega a ele
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let (read, release) = gate().unwrap();
        let fd = read.as_raw_fd();
        let [sh, flag, script, name] = GATE_SCRIPT;
        let mut command = std::process::Command::new(sh);
        command.args([flag, script, name, "sh", "-c", "[ -e /proc/self/fd/3 ] && echo leaked; echo \"$1\"", "x", "first arg"]);
        command.stdout(std::process::Stdio::piped());
        // SAFETY: só o dup2/fcntl entre o fork e o exec
        unsafe {
            command.pre_exec(move || gate_fd_in_child(fd));
        }
        let child = command.spawn().unwrap();
        drop(read);
        drop(release);
        let out = child.wait_with_output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "first arg\n");
    }

    #[test]
    fn the_launcher_args_are_only_ours() {
        let args = |v: &[&str]| v.iter().map(std::ffi::OsString::from).collect::<Vec<_>>();
        assert_eq!(launcher_main(&args(&["start"])), None);
        assert_eq!(launcher_main(&args(&[])), None);
        assert_eq!(launcher_main(&args(&[LAUNCH_ARG, "/nonexistent/x.desktop", ""])), Some(1));
    }

    #[test]
    fn the_app_waits_for_its_job_when_the_signal_comes_after_the_reply() {
        let mut wait = JobWait::default();
        assert!(!wait.on_reply(Some("/job/7")));
        assert!(!wait.on_removed("/job/6"), "another job released the app");
        assert!(wait.on_removed("/job/7"));
    }

    #[test]
    fn a_job_removed_before_the_reply_releases_at_the_reply() {
        let mut wait = JobWait::default();
        assert!(!wait.on_removed("/job/7"));
        assert!(wait.on_reply(Some("/job/7")));
    }

    #[test]
    fn another_jobs_signal_before_the_reply_does_not_release_the_app() {
        let mut wait = JobWait::default();
        assert!(!wait.on_removed("/job/6"));
        assert!(!wait.on_reply(Some("/job/7")), "released by another job's signal");
        assert!(wait.on_removed("/job/7"));
    }

    #[test]
    fn a_reply_without_a_job_does_not_hold_the_app() {
        assert!(JobWait::default().on_reply(None));
    }

    #[test]
    fn the_app_is_released_only_once() {
        let mut wait = JobWait::default();
        assert!(!wait.on_reply(Some("/job/7")));
        assert!(wait.give_up());
        assert!(!wait.on_removed("/job/7"), "released twice");
        assert!(!wait.give_up(), "released twice");
    }

    #[test]
    fn the_launcher_argv_keeps_an_empty_action_and_every_uri() {
        use std::ffi::OsString;
        use std::path::Path;
        let argv = launcher_argv(
            Path::new("/usr/bin/swaydesk-dock"),
            Path::new("/usr/share/applications/foot.desktop"),
            None,
            &["file:///tmp/a%20b.txt".to_string(), "file:///tmp/c".to_string()],
        );
        let want: Vec<OsString> = ["/usr/bin/swaydesk-dock", LAUNCH_ARG, "/usr/share/applications/foot.desktop", "",
            "file:///tmp/a%20b.txt", "file:///tmp/c"].iter().map(OsString::from).collect();
        assert_eq!(argv, want);
        let action = launcher_argv(Path::new("/b"), Path::new("/f.desktop"), Some("new-window"), &[]);
        assert_eq!(action[3], OsString::from("new-window"));
    }

    #[test]
    fn an_env_entry_splits_at_the_first_equals() {
        use std::ffi::{OsStr, OsString};
        assert_eq!(env_pair(OsStr::new("A=b=c")), Some((OsString::from("A"), OsString::from("b=c"))));
        assert_eq!(env_pair(OsStr::new("EMPTY=")), Some((OsString::from("EMPTY"), OsString::new())));
        assert_eq!(env_pair(OsStr::new("NOEQUALS")), None);
    }

    /// Uma pasta temporária que some no fim do teste, também quando ele falha.
    struct TempDir(std::path::PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_launcher_reaches_the_helper_with_its_args_intact() {
        // O gate de verdade, com um "binário" que só imprime os argumentos: nada se perde nem se junta no sh
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let dir = TempDir(std::env::temp_dir().join(format!("app-scope-test-{}", std::process::id())));
        std::fs::create_dir_all(&dir.0).unwrap();
        let exe = dir.0.join("fake-component");
        std::fs::write(&exe, "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]' \"$a\"; done\n").unwrap();
        std::fs::set_permissions(&exe, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        // Outro teste que crie um processo enquanto o arquivo ainda está aberto para escrita deixa o exec dele
        // com ETXTBSY ("Text file busy") por um instante: outra tentativa resolve
        let mut out = None;
        for _ in 0..20 {
            let (read, release) = gate().unwrap();
            let fd = read.as_raw_fd();
            let mut command = std::process::Command::new(GATE_SCRIPT[0]);
            command.args(&GATE_SCRIPT[1..]);
            command.args(launcher_argv(&exe, std::path::Path::new("/a b.desktop"), None, &["x y".to_string()]));
            command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
            // SAFETY: só o dup2/fcntl entre o fork e o exec
            unsafe {
                command.pre_exec(move || gate_fd_in_child(fd));
            }
            let child = command.spawn().unwrap();
            drop(read);
            drop(release);
            let result = child.wait_with_output().unwrap();
            if !String::from_utf8_lossy(&result.stderr).contains("busy") {
                out = Some(result);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let out = out.expect("the fake component stayed busy");
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("[{LAUNCH_ARG}][/a b.desktop][][x y]"));
    }

    #[test]
    fn outside_a_unit_nothing_is_called() {
        // Sem o INVOCATION_ID (o ambiente dos testes), nem o barramento é aberto
        if under_unit() {
            return; // um cargo test rodando dentro de um serviço
        }
        move_to_scope(1, "x.desktop");
    }
}
