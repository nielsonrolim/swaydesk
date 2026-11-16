//! Telas: a posição, a resolução, a taxa, a escala e a rotação de cada tela no Sway, e o serviço que aplica
//! o layout de cada conjunto de monitores ao plugar ou desplugar. O substituto do nwg-displays.
//!
//! uso: swaydesk-displays           a janela (abrir de novo traz a que já está aberta)
//!      swaydesk-displays daemon    o serviço de perfis (o laço do config do Sway o abre)
//!      swaydesk-displays apply     aplica o perfil do conjunto agora (o bindswitch da tampa)
//!      swaydesk-displays --check   só confere que o binário carrega
//!
//! Aplicar muda as telas na hora e pergunta se mantém; sem resposta em 15 s, volta ao que era. Manter
//! guarda o perfil do conjunto (~/.config/sway-desktop/displays.json) e o arquivo que o Sway lê no login
//! (~/.config/sway/config.d/90-displays.conf).
mod daemon;
mod identify;
mod sway;
mod window;

use std::cell::RefCell;
use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::{config, i18n, resident};

pub const PROGRAM: &str = "sway-displays";
/// Igual ao nome do .desktop: é como a dock acha o ícone, e como o swaydesk-help abre o app
const APP_ID: &str = "org.swaydesktop.Displays";

pub fn warn(message: &str) {
    eprintln!("{PROGRAM}: {message}"); // vai para o sway.log, junto com o resto da sessão
}

/// A trava que o app segura enquanto mostra a prévia: o serviço não aplica perfis por cima dela
fn preview_lock_path() -> PathBuf {
    resident::runtime_dir().join("sway-displays-preview.lock")
}

/// Segura a trava da prévia; None se outra janela já segura
pub fn take_preview_lock() -> Option<File> {
    resident::take_lock(&preview_lock_path(), Duration::ZERO)
}

/// Uma prévia está na tela?
pub fn preview_active() -> bool {
    take_preview_lock().is_none()
}

fn main() -> ExitCode {
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
    let window: Rc<RefCell<Option<Rc<window::Window>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        let existing = window.borrow().clone();
        match existing {
            Some(w) => w.window.present(),
            None => *window.borrow_mut() = Some(window::Window::new(app)),
        }
    });
    let code = app.run_with_args(&std::env::args().take(1).collect::<Vec<_>>());
    ExitCode::from(code.value() as u8)
}
