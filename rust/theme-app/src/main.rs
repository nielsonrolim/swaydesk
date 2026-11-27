//! O app Tema do Desktop: a janela da página da biblioteca (lib.rs). Abrir de novo traz a janela que já está
//! aberta.
use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::config;
use swaydesk_common::i18n;

const APP_ID: &str = "org.swaydesktop.Theme"; // igual ao nome do .desktop: é como a dock acha o ícone

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-theme-app [--settings-panel]\n",
    "    Opens Desktop Theme: the GTK theme family, light or dark, and the accent color.\n",
    "    --settings-panel opens the Appearance panel of Settings instead, when Settings is installed.",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    match std::env::args().nth(1).as_deref() {
        Some("--check") => return ExitCode::SUCCESS, // o sway-theme-app em Python confere que o binário carrega
        // O .desktop: com a Central instalada, o painel Aparência dela; sem, a janela daqui
        Some(swaydesk_common::settings_center::SETTINGS_PANEL_FLAG) => {
            swaydesk_settings_logic::exec_hand_off("appearance", "swaydesk-theme-app")
        }
        _ => {}
    }
    swaydesk_ui::use_cairo_renderer();
    // Sem isso, o app_id no Sway é o nome do programa, e a dock não acha o .desktop nem o ícone
    glib::set_prgname(Some(APP_ID));
    i18n::init(&config::load());
    swaydesk_common::theme_resources::register_theme_resources();
    let app = gtk4::Application::new(Some(APP_ID), gio::ApplicationFlags::default());
    let window: Rc<RefCell<Option<gtk4::ApplicationWindow>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        let existing = window.borrow().clone();
        match existing {
            Some(w) => w.present(),
            None => *window.borrow_mut() = Some(swaydesk_theme_app::window(app)),
        }
    });
    let code = app.run_with_args(&std::env::args().take(1).collect::<Vec<_>>());
    ExitCode::from(code.value() as u8)
}
