//! O app Telas: a janela da página da biblioteca (lib.rs), o serviço de perfis e o apply.
//!
//! uso: swaydesk-displays           a janela (abrir de novo traz a que já está aberta)
//!      swaydesk-displays daemon    o serviço de perfis (o laço do config do Sway o abre)
//!      swaydesk-displays apply     aplica o perfil do conjunto agora (o bindswitch da tampa)
//!      swaydesk-displays --check   só confere que o binário carrega
use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::{config, i18n};
use swaydesk_displays::{daemon, Displays};

/// Igual ao nome do .desktop: é como a dock acha o ícone, e como o swaydesk-help abre o app
const APP_ID: &str = "org.swaydesktop.Displays";

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-displays          opens Displays: position, resolution, rate, scale and rotation of each screen\n",
    "       swaydesk-displays daemon   the service that applies the saved layout when screens change\n",
    "       swaydesk-displays apply    applies the saved layout once",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    match std::env::args().nth(1).as_deref() {
        Some("--check") => return ExitCode::SUCCESS,
        Some("daemon") => return daemon::run(),
        Some("apply") => return daemon::apply_now(),
        _ => {}
    }
    swaydesk_common::renderer::use_cairo_renderer();
    // Sem isso, o app_id no Sway é o nome do programa, e a dock não acha o .desktop nem o ícone
    glib::set_prgname(Some(APP_ID));
    i18n::init(&config::load());
    swaydesk_common::theme_resources::register_theme_resources();
    let app = gtk4::Application::new(Some(APP_ID), gio::ApplicationFlags::default());
    // O adw-gtk3 num GTK antes do 4.20 deixa a janela clara até no escuro: o Adwaita embutido no lugar
    app.connect_startup(|_| swaydesk_ui::follow_theme_stand_in());
    let window: Rc<RefCell<Option<Displays>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        if let Some(w) = window.borrow().as_ref() {
            w.reopen();
            return;
        }
        *window.borrow_mut() = Some(swaydesk_displays::open(app));
    });
    // O SIGTERM (um pkill, o systemd), o SIGINT e o SIGHUP fecham as janelas: a que tem uma prévia sem resposta
    // desfaz no close-request, como no Super+Q
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let app = app.clone();
        glib::unix_signal_add_local(signal, move || {
            for window in app.windows() {
                window.close();
            }
            app.quit();
            glib::ControlFlow::Break
        });
    }
    let code = app.run_with_args(&std::env::args().take(1).collect::<Vec<_>>());
    ExitCode::from(code.value() as u8)
}
