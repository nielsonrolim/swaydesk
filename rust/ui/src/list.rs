//! O que os popups de listas têm em comum (o `ListPopup` e o `Options` do `waybar_popup.py`): o CSS dos
//! botões e das linhas, os botões de opção, a rolagem de altura fixa, os links e a linha de status.
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::glib;

/// Os sliders (brilho, volume).
pub const CSS_SCALE: &str = r#"
scale { padding: 0 4px; }
scale trough { min-height: 4px; border-radius: 2px; background: #26262a; border: none; }
scale highlight { background: @accent; border-radius: 2px; }
scale slider {
  min-width: 14px; min-height: 14px; margin: -6px; border-radius: 7px;
  background: #dadada; border: none; box-shadow: none;
}
"#;

/// Os botões de opção lado a lado, com o escolhido na cor de destaque.
pub const CSS_OPT: &str = r#"
button.opt {
  background: #38383d; border: none; box-shadow: none; color: #dadada;
  padding: 6px 4px; border-radius: 8px; min-height: 0; font-size: 13px;
}
button.opt:hover { background: #4c4c50; }
button.opt.active { background: @accent; color: @on_accent; }
button.opt.active:hover { background: @accent_hover; }
"#;

/// As linhas de aparelho (rede, Bluetooth), com o ativo na cor de destaque.
pub const CSS_DEV: &str = r#"
button.dev:hover { background: #4c4c50; }
button.dev.active { background: @accent; color: @on_accent; }
button.dev.active:hover { background: @accent_hover; }
.dev-name { font-size: 13px; }
.dev-sub  { font-size: 11px; color: #8a8a8f; }
"#;

/// As linhas, os interruptores, o botão de fechar (.off), o status, o vazio e a barra de rolagem.
pub const CSS_LIST: &str = r#"
button.dev {
  background: transparent; border: none; box-shadow: none; color: #dadada;
  padding: 5px 8px; border-radius: 8px; min-height: 0;
}
.toggle { font-size: 13px; }
.glyph { font-family: "Symbols Nerd Font"; font-size: 17px; min-width: 24px; }
switch { background: #26262a; border: none; border-radius: 10px; min-height: 18px; min-width: 34px; }
switch:checked { background: @accent; }
switch:disabled { opacity: 0.5; }
switch slider {
  background: #dadada; border: none; box-shadow: none; border-radius: 8px;
  min-width: 16px; min-height: 16px; margin: 1px;
}
button.dev.busy { background: #4c4c50; }
button.dev.active .dev-sub { color: @accent_soft; }
button.dev.busy .dev-sub { color: @busy_sub; }
button.off {
  background: transparent; border: none; box-shadow: none; color: #dadada;
  font-family: "Symbols Nerd Font"; font-size: 15px;
  padding: 0 8px; min-height: 0; border-radius: 8px;
}
button.off:hover { background: #4c4c50; color: #ffffff; }
.key { color: #8a8a8f; font-size: 12px; }
.value { font-size: 12px; }
.status { color: #f27835; font-size: 11px; }
.empty { color: #8a8a8f; font-size: 12px; padding: 4px; }
scrolledwindow undershoot, scrolledwindow overshoot { background: none; border: none; box-shadow: none; }
scrollbar { background: transparent; border: none; }
scrollbar slider { background: #4c4c50; border: none; border-radius: 3px; min-width: 4px; min-height: 20px; }
"#;

/// A altura da rolagem dos popups de listas.
pub const SCROLL_HEIGHT: i32 = 400;

/// Botões lado a lado com o escolhido na cor de destaque (usa o CSS_OPT).
pub struct Options {
    pub widget: gtk4::Box,
    pub buttons: HashMap<String, gtk4::Button>,
}

impl Options {
    pub fn new(items: &[(&str, String)], on_pick: impl Fn(&str) + 'static) -> Options {
        let widget = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        widget.set_homogeneous(true);
        let on_pick = Rc::new(on_pick);
        let mut buttons = HashMap::new();
        for (key, text) in items {
            let button = gtk4::Button::with_label(text);
            button.add_css_class("opt");
            let (pick, on_pick) = (key.to_string(), on_pick.clone());
            button.connect_clicked(move |_| on_pick(&pick));
            widget.append(&button);
            buttons.insert(key.to_string(), button);
        }
        Options { widget, buttons }
    }

    /// Liga ou desliga todos os botões (sem o programa, por exemplo).
    pub fn set_sensitive(&self, sensitive: bool) {
        for button in self.buttons.values() {
            button.set_sensitive(sensitive);
        }
    }

    /// Marca o botão de `key`; uma chave que não existe (como "") desmarca todos.
    pub fn select(&self, key: &str) {
        for (k, button) in &self.buttons {
            if k == key {
                button.add_css_class("active");
            } else {
                button.remove_css_class("active");
            }
        }
    }
}

/// Um rótulo e um interruptor, lado a lado em `row` (o `make_switch` do Python).
pub fn switch(row: &gtk4::Box, title: &str) -> gtk4::Switch {
    let label = gtk4::Label::new(Some(title));
    label.add_css_class("toggle");
    let sw = gtk4::Switch::new();
    sw.set_valign(gtk4::Align::Center);
    row.append(&label);
    row.append(&sw);
    sw
}

/// Uma seção da lista: o título e as linhas numa caixa (o `add_section` do Python).
pub fn section(title: &str, rows: &[gtk4::Widget]) -> gtk4::Box {
    let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    sec.append(&crate::label(title, &["section"], 0.0));
    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    body.add_css_class("box");
    for row in rows {
        body.append(row);
    }
    sec.append(&body);
    sec
}

/// Rolagem só na vertical, de altura fixa: mudar o tamanho da superfície faz o Sway tirar o foco.
pub fn scroller(child: &impl IsA<gtk4::Widget>, height: i32) -> gtk4::ScrolledWindow {
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_min_content_height(height);
    scroll.set_max_content_height(height);
    scroll.set_child(Some(child));
    scroll
}

/// Um link (o `button.link` do CSS comum), com a dica.
pub fn link_button(title: &str, tip: &str, on_click: impl Fn() + 'static) -> gtk4::Button {
    let button = gtk4::Button::with_label(title);
    button.add_css_class("link");
    button.set_tooltip_text(Some(tip));
    button.connect_clicked(move |_| on_click());
    button
}

pub fn empty_label(text: &str) -> gtk4::Label {
    crate::label(text, &["empty"], 0.0)
}

/// A linha de status: sempre com texto, para o tamanho não mudar; a dica mostra o texto inteiro.
pub fn set_status(status: &gtk4::Label, text: &str) {
    status.set_text(if text.is_empty() { " " } else { text });
    status.set_tooltip_text(if text.is_empty() { None } else { Some(text) });
}

/// A mensagem sem o prefixo "GDBus.Error:nome.do.erro: ".
pub fn error_text(err: &glib::Error) -> String {
    swaydesk_common::text::strip_remote_error(err.message()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_is_ascii() {
        for css in [CSS_SCALE, CSS_OPT, CSS_DEV, CSS_LIST] {
            assert!(css.is_ascii());
        }
    }
}
