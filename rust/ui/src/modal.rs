//! O cartão no centro da tela sobre um fundo escurecido: o porte do `ModalPanel` do
//! `waybar_popup.py`, usado pelo menu da sessão e pelo `sway-capture`.
//!
//! A janela cobre a tela inteira (barra incluída) na camada OVERLAY, com o teclado exclusivo. O
//! conteúdo vai em [`ModalPanel::card`]. Clicar fora do cartão ou Esc fecha; as outras teclas ficam
//! com o programa, num `EventControllerKey` próprio na [`ModalPanel::window`]. Rode com o
//! [`crate::run_single`]. Nos testes, abra só num Sway isolado e com prazo: o teclado exclusivo
//! prenderia a sessão de quem está testando.
use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

pub struct ModalPanel {
    pub program: &'static str,
    pub window: gtk4::Window,
    /// A caixa vertical do cartão (classe `card`): o CSS do programa dá a cor e o espaçamento interno
    pub card: gtk4::Box,
    main_loop: glib::MainLoop,
    closing: Cell<bool>,
}

impl ModalPanel {
    /// `css`: o do programa, depois das cores e da fonte da config (como o `THEME_CSS + css` do Python:
    /// o fundo escurecido e o `.card` vêm dele).
    pub fn new(program: &'static str, namespace: &str, css: &str, card_spacing: i32, main_loop: &glib::MainLoop) -> Rc<ModalPanel> {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(&format!("{}{css}", crate::theme_css()));
        if let Some(display) = gdk::Display::default() {
            #[allow(deprecated)] // sem substituto no GTK 4.18
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        crate::always_dark();
        if let Some(settings) = gtk4::Settings::default() {
            settings.set_gtk_cursor_blink(false);
        }

        let window = gtk4::Window::new();
        if gtk4_layer_shell::is_supported() {
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                window.set_anchor(edge, true);
            }
            window.set_exclusive_zone(-1); // cobre a barra também
            window.set_keyboard_mode(KeyboardMode::Exclusive);
            window.set_namespace(namespace);
        } else {
            // X11 e GNOME Shell não têm o protocolo: janela comum em tela cheia
            window.set_decorated(false);
            window.fullscreen();
        }

        // O fundo preenche a janela; o cartão fica no centro. O clique fecha só fora do cartão
        let background = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        background.set_hexpand(true);
        background.set_vexpand(true);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, card_spacing);
        card.add_css_class("card");
        card.set_halign(gtk4::Align::Center);
        card.set_valign(gtk4::Align::Center);
        card.set_vexpand(true);
        background.append(&card);
        window.set_child(Some(&background));

        let panel = Rc::new(ModalPanel {
            program,
            window,
            card,
            main_loop: main_loop.clone(),
            closing: Cell::new(false),
        });

        let click = gtk4::GestureClick::new();
        click.set_button(0);
        let weak = Rc::downgrade(&panel);
        click.connect_pressed(move |gesture, _, x, y| {
            let Some(p) = weak.upgrade() else { return };
            let Some(area) = gesture.widget() else { return };
            // O widget sob o ponteiro: dentro do cartão (um botão, o próprio cartão) não fecha
            let inside = area.pick(x, y, gtk4::PickFlags::DEFAULT).is_some_and(|w| w == p.card || w.is_ancestor(&p.card));
            if !inside {
                p.close("clique fora");
            }
        });
        background.add_controller(click);

        let keys = gtk4::EventControllerKey::new();
        let weak = Rc::downgrade(&panel);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(p) = weak.upgrade() {
                    p.close("Esc");
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        panel.window.add_controller(keys);
        panel.window.connect_close_request({
            let weak = Rc::downgrade(&panel);
            move |_| {
                if let Some(p) = weak.upgrade() {
                    p.close("janela fechada");
                }
                glib::Propagation::Proceed
            }
        });
        panel
    }

    pub fn show(&self) {
        self.window.set_visible(true);
        self.window.present();
    }

    /// Se já está fechando: uma segunda ativação no mesmo ciclo do laço (Enter com repetição, uma tecla
    /// e um clique juntos) não deve repetir a ação.
    pub fn is_closing(&self) -> bool {
        self.closing.get()
    }

    /// Fecha (sai do laço do [`crate::run_single`]); o motivo vai para o `waybar-popups.log`.
    pub fn close(&self, reason: &str) {
        if self.closing.replace(true) {
            return;
        }
        crate::log(self.program, reason);
        self.main_loop.quit();
    }
}
