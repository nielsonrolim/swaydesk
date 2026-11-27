//! Telas: a posição, a resolução, a taxa, a escala e a rotação de cada tela no Sway, e o serviço que aplica
//! o layout de cada conjunto de monitores ao plugar ou desplugar. O substituto do nwg-displays.

//! (Os comandos do binário estão no main.rs.)
//!
//! Aplicar muda as telas na hora e pergunta se mantém; sem resposta em 15 s, volta ao que era. Manter
//! guarda o perfil do conjunto (~/.config/swaydesk/displays.json) e o arquivo que o Sway lê no login
//! (~/.config/sway/config.d/90-displays.conf).
//!
//! A mesma página serve a janela do app ([`open`]) e o painel Telas da Central de Configurações ([`panel`],
//! ligado estaticamente: a Central não depende do pacote do app). A trava da prévia é um arquivo no
//! XDG_RUNTIME_DIR: com a janela e o painel abertos (dois processos), só um mostra a prévia por vez.
pub mod daemon;
mod identify;
mod sway;
mod window;

use std::fs::File;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use swaydesk_common::{i18n::tr, resident};

pub const PROGRAM: &str = "sway-displays";
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

/// A janela do app Telas, com a página dentro.
pub struct Displays(Rc<window::Page>);

impl Displays {
    /// Abrir de novo (a janela já estava aberta): traz a janela e escolhe a tela com o foco
    pub fn reopen(&self) {
        self.0.reopen();
    }
}

/// Abre a janela do app.
pub fn open(app: &gtk4::Application) -> Displays {
    let app_window = gtk4::ApplicationWindow::new(app);
    app_window.set_title(Some(&tr("Displays")));
    app_window.set_default_size(780, 600);
    let page = window::Page::new(Some(&app_window));
    app_window.present();
    page.focus();
    Displays(page)
}

/// O painel Telas da Central. Cada vez que aparece, relê as telas (um monitor plugado ou uma mudança feita no
/// app com o painel escondido), sem perder o que ainda não foi aplicado no mesmo conjunto.
pub fn panel() -> gtk4::Widget {
    let page = window::Page::new(None);
    let root = page.root.clone();
    // O closure segura a página enquanto o widget existe (o dispose do GTK solta os closures no destroy)
    root.connect_map(move |_| page.on_shown());
    root.upcast()
}
