//! O que o toast e o popup das notificações têm em comum: o ícone, o texto, os botões e o CSS (a parte
//! de cima de `waybar_notifications_gtk.py`).
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gdk_pixbuf, pango};
use swaydesk_ui::label;
use swaydesk_ui::list::{CSS_DEV, CSS_LIST, CSS_OPT};

pub const ICON_SIZE: i32 = 40;
pub const BODY_LINES: i32 = 3;
/// Sem limite, um texto longo alargaria a janela
pub const TEXT_CHARS: i32 = 38;

pub const ICON_BELL: &str = "\u{f009a}"; // bell (Material da Nerd Font)
pub const ICON_CLOSE: &str = "\u{f0156}"; // close

/// O CSS do texto e do cartão das notificações, depois do CSS_OPT e do CSS_LIST do swaydesk-ui.
pub const CSS_NOTIF: &str = r#"
.notif-summary { font-size: 13px; font-weight: bold; }
.notif-body { font-size: 12px; }
.notif-app { color: #8a8a8f; font-size: 11px; font-weight: bold; }
.notif-time { color: #8a8a8f; font-size: 11px; }
.card.critical { border-color: #fc4138; }
button.opt.action { padding: 4px 8px; font-size: 12px; font-weight: bold; } /* como o Adwaita do GTK 3 */
"#;

/// Todo o CSS das notificações, para o toast e o popup.
pub fn css() -> String {
    format!("{CSS_OPT}{CSS_DEV}{CSS_LIST}{CSS_NOTIF}")
}

/// Caminho absoluto ou nome no tema; o que não abrir vira o sino.
pub fn make_icon(spec: &str, size: i32) -> gtk4::Widget {
    let mut image: Option<gtk4::Image> = None;
    if spec.starts_with('/') {
        if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file_at_scale(spec, size, size, true) {
            let texture = gdk::Texture::for_pixbuf(&pixbuf);
            let img = gtk4::Image::from_paintable(Some(&texture));
            img.set_pixel_size(size);
            image = Some(img);
        }
    } else if !spec.is_empty() {
        let has = gdk::Display::default().is_some_and(|d| gtk4::IconTheme::for_display(&d).has_icon(spec));
        if has {
            let img = gtk4::Image::from_icon_name(spec);
            img.set_pixel_size(size);
            image = Some(img);
        }
    }
    let widget: gtk4::Widget = match image {
        Some(img) => img.upcast(),
        None => label(ICON_BELL, &["glyph"], 0.5).upcast(),
    };
    widget.set_size_request(size, size);
    widget.set_valign(gtk4::Align::Start);
    widget
}

/// O corpo já vem limpo; a conferência é a última proteção contra marcação que o Pango recuse.
pub fn set_body(lbl: &gtk4::Label, markup: &str) {
    if pango::parse_markup(markup, '\0').is_ok() {
        lbl.set_markup(markup);
    } else {
        lbl.set_text(markup);
    }
}

/// Título e até BODY_LINES linhas de corpo.
pub fn make_text(summary: &str, body: &str, width_chars: i32) -> gtk4::Box {
    let col = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    col.set_hexpand(true);
    if !summary.is_empty() {
        let title = label(summary, &["notif-summary"], 0.0);
        title.set_ellipsize(pango::EllipsizeMode::End);
        title.set_max_width_chars(width_chars);
        col.append(&title);
    }
    if !body.is_empty() {
        let text = gtk4::Label::new(None);
        text.set_xalign(0.0);
        text.add_css_class("notif-body");
        text.set_wrap(true);
        text.set_wrap_mode(pango::WrapMode::WordChar);
        text.set_lines(BODY_LINES);
        text.set_ellipsize(pango::EllipsizeMode::End);
        text.set_max_width_chars(width_chars);
        set_body(&text, body);
        col.append(&text);
    }
    col
}

/// Os botões de ação; None se o app não oferece nenhum.
pub fn make_buttons(buttons: &[(String, String)], on_click: impl Fn(&str) + 'static) -> Option<gtk4::Box> {
    if buttons.is_empty() {
        return None;
    }
    let on_click = Rc::new(on_click);
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    row.set_homogeneous(true);
    for (key, text) in buttons {
        let button = gtk4::Button::with_label(text);
        button.add_css_class("opt");
        button.add_css_class("action"); // só os de ação: os do não perturbe seguem o CSS_OPT
        button.set_can_focus(false);
        let (key, on_click) = (key.clone(), on_click.clone());
        button.connect_clicked(move |_| on_click(&key));
        row.append(&button);
    }
    Some(row)
}

/// Toast não leva tooltip: o layout muda e a janela some sob o cursor parado.
pub fn make_close(on_click: impl Fn() + 'static, tooltip: Option<&str>) -> gtk4::Button {
    let button = gtk4::Button::with_label(ICON_CLOSE);
    button.add_css_class("off");
    button.set_can_focus(false);
    button.set_valign(gtk4::Align::Start);
    button.set_tooltip_text(tooltip);
    button.connect_clicked(move |_| on_click());
    button
}
