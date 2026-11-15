//! Dock inferior do Sway: o porte de `home/.local/bin/sway-dock` para GTK 4.
//!
//! uso: sway-dock              abre a dock (uma por sessão)
//!      sway-dock reload       relê ~/.config/sway-dock/config.toml
//!      sway-dock pin|unpin ID pede à dock no ar que fixe ou tire um app (o id do .desktop); a
//!                             grade de apps usa. Sem a dock, sai com 1 e não muda nada
//!
//! Os arquivos são os da versão em Python (PID, trava, fixados, a fila de pedidos e a lista publicada
//! para a grade), e o processo roda com o nome `sway-dock` no argv[0] e no comm: o `sway-theme`, a
//! grade e o `sway-dock pin` o acham do mesmo jeito.
mod dock;

use std::cell::RefCell;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk4::glib;
use swaydesk_common::{config, i18n, resident};
use swaydesk_dock_logic as dl;

pub const PROGRAM: &str = "sway-dock";

pub fn warn(message: &str) {
    eprintln!("{PROGRAM}: {message}"); // vai para o sway.log, junto com o resto da sessão
}

pub fn requests_path() -> PathBuf {
    resident::runtime_dir().join(format!("{PROGRAM}.requests"))
}

/// Roda de novo com o nome `sway-dock` no argv[0], que é o que o /proc mostra a quem confere o arquivo
/// de PID. Só volta se não conseguir.
fn reexec_as_program() {
    let Ok(exe) = std::env::current_exe() else { return };
    let err = std::process::Command::new(exe).arg0(PROGRAM).args(std::env::args_os().skip(1)).exec();
    warn(&format!("could not run as {PROGRAM}: {err}"));
}

fn tr(msgid: &str) -> String {
    i18n::init(&config::load());
    i18n::tr(msgid)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = resident::runtime_dir();
    // Fixar pela grade de apps: antes de carregar o GTK
    if let [action, key] = args.as_slice() {
        if dl::PIN_ACTIONS.contains(&action.as_str()) {
            return match dl::pin_command(action, key, &requests_path(), &run) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => {
                    eprintln!("{PROGRAM}: the dock is not running; nothing changed");
                    ExitCode::FAILURE
                }
                Err(err) => {
                    eprintln!("{PROGRAM}: could not send the request: {err}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    let pidfile = dl::pidfile(&run);
    match args.as_slice() {
        [] => {}
        // O sway-dock em Python confere, antes de passar a vez, que o binário carrega: sem uma
        // biblioteca (o gtk4-layer-shell), quem falha é o carregador, antes de chegar aqui
        [arg] if arg == "--check" => return ExitCode::SUCCESS,
        [arg] if arg == "reload" => {
            if resident::signal_pid(&pidfile, PROGRAM, libc::SIGUSR1) {
                return ExitCode::SUCCESS;
            }
            eprintln!("{}", tr("sway-dock is not running"));
            return ExitCode::FAILURE;
        }
        _ => {
            eprintln!("{}", tr("usage: sway-dock [reload | pin ID | unpin ID]"));
            return ExitCode::from(2);
        }
    }

    // O pgrep -x e o pkill -x do sway-theme procuram sway-dock no comm
    resident::set_process_name(PROGRAM);
    let argv0 = std::env::args_os().next().unwrap_or_default();
    if Path::new(&argv0).file_name() != Some(PROGRAM.as_ref()) {
        reexec_as_program();
    }
    // O renderizador cairo, salvo se o ambiente escolher outro: no Sway headless, a dock ocupava 136 MB de
    // RSS com o GL (NGL) e ocupa 36 MB com o cairo (a versão em Python, 63 MB), e ela fica aberta a sessão
    // inteira. Antes dos sinais do GLib, do GTK e de qualquer outra linha: o set_var só é seguro aqui. Os
    // apps que ela abre não herdam a escolha (cmd::clean_launch_context e cmd::spawn_detached)
    swaydesk_ui::use_cairo_renderer();
    let Some(_lock) = resident::take_lock(&resident::lockfile(PROGRAM), Duration::ZERO) else {
        eprintln!("{}", tr("sway-dock is already running"));
        return ExitCode::FAILURE;
    };

    // Os sinais antes do arquivo do PID: um reload ou um pedido de fixar durante a subida mataria a dock
    // com a ação padrão. O GLib os guarda até o laço rodar, com a dock pronta (o pedido fica na fila)
    let main_loop = glib::MainLoop::new(None, false);
    let holder: Rc<RefCell<Option<Rc<dock::App>>>> = Rc::new(RefCell::new(None));
    for signal in [libc::SIGTERM, libc::SIGINT] {
        let main_loop = main_loop.clone();
        glib::unix_signal_add_local(signal, move || {
            main_loop.quit();
            glib::ControlFlow::Continue
        });
    }
    let handlers: [(i32, fn(&dock::App)); 3] =
        [(libc::SIGUSR1, dock::App::reload), (libc::SIGUSR2, dock::App::dump), (libc::SIGHUP, dock::App::pin_requests)];
    for (signal, handler) in handlers {
        let holder = holder.clone();
        glib::unix_signal_add_local(signal, move || {
            let app = holder.borrow().clone();
            if let Some(app) = app {
                handler(&app);
            }
            glib::ControlFlow::Continue
        });
    }
    // Pedidos que sobraram de uma dock que saiu sem tratá-los: velhos, não são aplicados de surpresa.
    // Antes do arquivo do PID: até ali, ninguém enfileira (a dock não conta como no ar)
    match dl::take_requests(&requests_path()) {
        Ok(stale) if !stale.is_empty() => warn(&format!("dropped {} pin request(s) left by a previous dock", stale.len())),
        Ok(_) => {}
        Err(err) => warn(&format!("could not read the pin requests in {}: {err}", requests_path().display())),
    }
    // A lista da dock anterior sai: a grade só oferece a opção quando esta dock publicar a dela
    if let Err(err) = dl::remove_pinned_ids(&run) {
        warn(&format!("could not remove the old list of pinned apps: {err}"));
    }
    if let Err(err) = resident::write_pid(&pidfile) {
        warn(&format!("could not write {}: {err}", pidfile.display()));
    }

    let code = run_dock(&holder, &main_loop);
    if let Some(app) = holder.borrow_mut().take() {
        app.shutdown();
    }
    resident::remove_own_pid(&pidfile, PROGRAM);
    code
}

fn run_dock(holder: &Rc<RefCell<Option<Rc<dock::App>>>>, main_loop: &glib::MainLoop) -> ExitCode {
    swaydesk_common::theme_resources::register_theme_resources();
    if let Err(err) = gtk4::init() {
        warn(&format!("could not start GTK: {err}"));
        return ExitCode::FAILURE;
    }
    i18n::init(&config::load());
    let app = dock::App::new(main_loop);
    *holder.borrow_mut() = Some(app.clone());
    main_loop.run();
    ExitCode::from(app.exit_code())
}
