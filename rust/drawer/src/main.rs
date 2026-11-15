//! Grade de apps em tela cheia: o porte de `home/.local/bin/sway-drawer` para GTK 4.
//!
//! uso: sway-drawer start    processo residente e escondido, no login
//!      sway-drawer          mostra ou esconde; sem o residente, vira o residente já aberto
//!
//! Os arquivos de PID e de trava são os da versão em Python, e o processo sempre roda com o nome
//! `sway-drawer` no argv[0]: o `sway-super`, a dock e o `sway-theme` o acham do mesmo jeito.
mod drawer;

use std::cell::RefCell;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk4::glib;
use swaydesk_common::{config, i18n, resident};

pub const PROGRAM: &str = "sway-drawer";
/// O start espera o residente antigo, que pode estar saindo, soltar a trava.
const START_WAIT: Duration = Duration::from_secs(3);

pub fn warn(message: &str) {
    eprintln!("{PROGRAM}: {message}"); // vai para o sway.log, junto com o resto da sessão
}

/// Roda de novo com o nome `sway-drawer` no argv[0], que é o que o /proc mostra a quem confere o
/// arquivo de PID. Só volta se não conseguir.
fn reexec_as_program() {
    let Ok(exe) = std::env::current_exe() else { return };
    let err = std::process::Command::new(exe).arg0(PROGRAM).args(std::env::args_os().skip(1)).exec();
    warn(&format!("could not run as {PROGRAM}: {err}"));
}

/// O Sway aberto direto do tty (sem tela de login nem o sway-session) não exporta o XDG_CURRENT_DESKTOP, e
/// com ele vazio o should_show() do Gio esconde os apps com OnlyShowIn (o Tema do Desktop é só do Sway e
/// dos outros compositores). Dentro do Sway (o SWAYSOCK existe), a grade assume "sway"; os apps que ela
/// abre herdam o valor, que é o certo para eles. Antes de criar linhas.
fn assume_sway_desktop() {
    let empty = std::env::var_os("XDG_CURRENT_DESKTOP").map_or(true, |v| v.is_empty());
    let in_sway = std::env::var_os("SWAYSOCK").is_some_and(|v| !v.is_empty());
    if empty && in_sway {
        std::env::set_var("XDG_CURRENT_DESKTOP", "sway");
    }
}

fn main() -> ExitCode {
    // O pgrep -x e o pkill -x do sway-theme e do sway-desktop-config procuram sway-drawer no comm
    resident::set_process_name(PROGRAM);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let start = match args.as_slice() {
        [] => false,
        [arg] if arg == "start" => true,
        // O sway-drawer em Python confere, antes de passar a vez, que o binário carrega: sem uma
        // biblioteca (o gtk4-layer-shell), quem falha é o carregador, antes de chegar aqui
        [arg] if arg == "--check" => return ExitCode::SUCCESS,
        _ => {
            eprintln!("{}", i18n::tr("usage: sway-drawer [start]"));
            return ExitCode::from(2);
        }
    };
    let pidfile = resident::pidfile(PROGRAM);
    // O toggle com o residente no ar só manda o sinal, antes de carregar qualquer coisa
    if !start && resident::signal_pid(&pidfile, PROGRAM, libc::SIGUSR1) {
        return ExitCode::SUCCESS;
    }
    let argv0 = std::env::args_os().next().unwrap_or_default();
    if Path::new(&argv0).file_name() != Some(PROGRAM.as_ref()) {
        reexec_as_program();
    }
    assume_sway_desktop();
    // Depois do re-exec: o processo novo herdaria a variável e a tomaria como vinda do ambiente
    swaydesk_common::renderer::use_cairo_renderer(); // a grade parada não precisa do GL; ~25 MB a menos
    let Some(_lock) = resident::take_lock(&resident::lockfile(PROGRAM), if start { START_WAIT } else { Duration::ZERO })
    else {
        return ExitCode::SUCCESS; // outro está subindo ou já roda
    };

    // Os sinais antes do arquivo do PID: um Super durante a subida mataria o processo com a ação
    // padrão do SIGUSR1. O GLib guarda o sinal até o laço rodar, com a grade pronta
    let main_loop = glib::MainLoop::new(None, false);
    let holder: Rc<RefCell<Option<Rc<drawer::Drawer>>>> = Rc::new(RefCell::new(None));
    for signal in [libc::SIGTERM, libc::SIGINT] {
        let main_loop = main_loop.clone();
        glib::unix_signal_add_local(signal, move || {
            main_loop.quit();
            glib::ControlFlow::Continue
        });
    }
    {
        let holder = holder.clone();
        glib::unix_signal_add_local(libc::SIGUSR1, move || {
            let drawer = holder.borrow().clone();
            if let Some(drawer) = drawer {
                drawer.toggle();
            }
            glib::ControlFlow::Continue
        });
    }
    {
        // A dock esconde a grade antes de abrir um app ou um menu
        let holder = holder.clone();
        glib::unix_signal_add_local(libc::SIGUSR2, move || {
            let drawer = holder.borrow().clone();
            if let Some(drawer) = drawer {
                drawer.hide();
            }
            glib::ControlFlow::Continue
        });
    }
    if let Err(err) = resident::write_pid(&pidfile) {
        warn(&format!("could not write {}: {err}", pidfile.display()));
    }

    let code = run(start, &holder, &main_loop);
    drawer::remove_shown();
    resident::remove_own_pid(&pidfile, PROGRAM);
    code
}

fn run(start: bool, holder: &Rc<RefCell<Option<Rc<drawer::Drawer>>>>, main_loop: &glib::MainLoop) -> ExitCode {
    swaydesk_common::theme_resources::register_theme_resources();
    if let Err(err) = gtk4::init() {
        warn(&format!("could not start GTK: {err}"));
        return ExitCode::FAILURE;
    }
    let config = config::load();
    i18n::init(&config);
    let drawer = drawer::Drawer::new(&config);
    *holder.borrow_mut() = Some(drawer.clone());
    if !start {
        drawer.show(); // sem o residente, quem pediu a grade a vê já
    }
    main_loop.run();
    ExitCode::SUCCESS
}
