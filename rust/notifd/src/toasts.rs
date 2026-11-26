//! A janela dos toasts (o porte da parte de baixo de `waybar_notifications_gtk.py`).
//!
//! Os toasts ficam em uma janela só, que nunca recebe o teclado e some quando fica vazia.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use gtk4::prelude::*;
use gtk4::{gdk, glib, pango};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use swaydesk_common::config;
use swaydesk_notifd::widgets::{css, make_buttons, make_close, make_icon, make_text, ICON_SIZE, TEXT_CHARS};
use swaydesk_notifd_logic::{Notification, CRITICAL};
use swaydesk_ui::{label, BASE_CSS};

use crate::{guard, Service, View};

const CARD_WIDTH: i32 = 360;
/// A distância da borda direita, a mesma da central
const RIGHT_MARGIN: i32 = 6;

struct Card {
    widget: gtk4::Box,
    revision: u64,
}

fn card(item: &Notification, service: &Weak<Service>) -> gtk4::Box {
    let card = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    card.add_css_class("card");
    if item.urgency == CRITICAL {
        card.add_css_class("critical");
    }
    card.set_size_request(CARD_WIDTH, -1);

    let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    top.append(&make_icon(&item.icon, ICON_SIZE));
    let col = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    col.set_hexpand(true);
    if !item.app_name.is_empty() {
        let app = label(&item.app_name, &["notif-app"], 0.0);
        app.set_ellipsize(pango::EllipsizeMode::End);
        app.set_max_width_chars(TEXT_CHARS);
        col.append(&app);
    }
    col.append(&make_text(&item.summary, &item.body, TEXT_CHARS));
    top.append(&col);
    let id = item.id;
    let s = service.clone();
    // Os cliques chegam ao serviço depois do evento: o card sai da tela no meio dele
    top.append(&make_close(
        move || {
            let s = s.clone();
            glib::idle_add_local_once(move || {
                if let Some(s) = s.upgrade() {
                    guard("toast close", || s.on_toast_close(id));
                }
            });
        },
        None,
    ));
    card.append(&top);

    let s = service.clone();
    if let Some(buttons) = make_buttons(&item.buttons(), move |key| {
        let (s, key) = (s.clone(), key.to_string());
        glib::idle_add_local_once(move || {
            if let Some(s) = s.upgrade() {
                guard("toast action", || s.invoke(id, &key));
            }
        });
    }) {
        card.append(&buttons);
    }

    // Só o botão esquerdo, apertado e solto aqui: soltar fora cancela, como em um botão. Os botões de
    // dentro ficam com o clique deles
    let click = gtk4::GestureClick::new();
    click.set_button(gdk::BUTTON_PRIMARY);
    let pressed = Rc::new(Cell::new(false));
    let p = pressed.clone();
    click.connect_pressed(move |_, _, _, _| p.set(true));
    let p = pressed.clone();
    click.connect_stopped(move |_| p.set(false));
    let s = service.clone();
    click.connect_released(move |gesture, _, x, y| {
        let Some(widget) = gesture.widget() else { return };
        let inside = 0.0 <= x && x < widget.width() as f64 && 0.0 <= y && y < widget.height() as f64;
        if pressed.replace(false) && inside {
            if let Some(s) = s.upgrade() {
                // Depois do evento: o card pode sair da tela no meio dele
                glib::idle_add_local_once(move || {
                    guard("toast body", || s.on_toast_body(id));
                });
            }
        }
    });
    card.add_controller(click);
    card
}

fn system_font_family() -> Option<String> {
    let name = gtk4::Settings::default()?.gtk_font_name()?;
    pango::FontDescription::from_string(&name).family().map(|f| f.to_string())
}

/// A janela dos toasts: o mais novo em cima, no canto superior direito.
pub struct ToastWindow {
    window: gtk4::Window,
    stack: gtk4::Box,
    cards: RefCell<HashMap<u32, Card>>,
    service: Weak<Service>,
}

impl ToastWindow {
    pub fn new(service: Weak<Service>) -> ToastWindow {
        let palette = config::palette(&config::load());
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(&format!(
            "{}{}{BASE_CSS}{}",
            config::accent_css(&palette),
            config::font_css(system_font_family().as_deref()),
            css()
        ));
        if let Some(display) = gdk::Display::default() {
            #[allow(deprecated)] // sem substituto no GTK 4.18
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        if let Some(settings) = gtk4::Settings::default() {
            settings.set_gtk_application_prefer_dark_theme(true);
        }
        let window = gtk4::Window::new();
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Right, true);
        window.set_margin(Edge::Top, 4);
        window.set_margin(Edge::Right, RIGHT_MARGIN);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_namespace("sway-notifd");
        let stack = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        window.set_child(Some(&stack));
        ToastWindow { window, stack, cards: RefCell::new(HashMap::new()), service }
    }
}

impl View for ToastWindow {
    fn set_right_offset(&self, offset: i32) {
        self.window.set_margin(Edge::Right, RIGHT_MARGIN + offset);
    }

    /// Refaz só o que mudou: uma bateria que substitui a própria notificação não pisca.
    ///
    /// Roda com a lista do serviço emprestada: nenhum callback daqui pode chegar ao serviço de forma
    /// síncrona (os cliques passam por um idle).
    fn render(&self, items: &[&Notification]) {
        let mut cards = self.cards.borrow_mut();
        cards.retain(|id, card| {
            let keep = items.iter().any(|i| i.id == *id && i.revision == card.revision);
            if !keep {
                self.stack.remove(&card.widget);
            }
            keep
        });
        // O mais novo em cima
        let mut previous: Option<gtk4::Box> = None;
        for item in items.iter().rev() {
            let card = cards.entry(item.id).or_insert_with(|| {
                let widget = card(item, &self.service);
                self.stack.append(&widget);
                Card { widget, revision: item.revision }
            });
            self.stack.reorder_child_after(&card.widget, previous.as_ref());
            previous = Some(card.widget.clone());
        }
        if items.is_empty() {
            self.window.set_visible(false); // ao reaparecer, sai na saída que estiver focada
        } else {
            self.window.set_default_size(1, 1); // sem isso a janela não encolhe quando um toast sai
            self.window.set_visible(true);
        }
    }
}
