//! O tamanho do cursor: uma lista com os tamanhos que o tema do cursor tem (lidos do left_ptr dele), aplicada ao
//! escolher, sem o botão Aplicar, pelo mesmo apply_cursor_size do `sway-theme cursor`. Fica fora da escolha do tema
//! (choice.rs): o tamanho não depende dela, e a trava do sway-theme põe as duas aplicações em fila.
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::config::{self, load_config, Config};
use swaydesk_common::cursor::{closest_size, theme_sizes};
use swaydesk_common::i18n::tr;
use swaydesk_theme::fill_one;
use swaydesk_theme::themes as st;

/// O texto da aplicação: as notas, ou o que não mudou.
fn message(result: Result<st::Applied, swaydesk_theme::Error>) -> (bool, String) {
    match result {
        Ok(applied) if applied.failures.is_empty() => (true, applied.notes.join("\n")),
        Ok(applied) => (false, fill_one(&tr("Could not update: %s"), &applied.failures.join(", "))),
        Err(err) => (false, err.to_string()),
    }
}

pub struct CursorSize {
    pub row: gtk4::Box,
    pub note: gtk4::Label,
    model: gtk4::StringList,
    dropdown: gtk4::DropDown,
    sizes: RefCell<Vec<u32>>,
    /// Enchendo a lista (o load): a troca da seleção não é uma escolha
    updating: Cell<bool>,
    applying: Cell<bool>,
}

impl CursorSize {
    pub fn new() -> Rc<CursorSize> {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        let label = gtk4::Label::new(Some(&tr("Cursor size")));
        label.set_xalign(0.0);
        label.add_css_class("theme-section");
        row.append(&label);
        let model = gtk4::StringList::new(&[]);
        let dropdown = gtk4::DropDown::new(Some(model.clone()), None::<gtk4::Expression>);
        label.set_mnemonic_widget(Some(&dropdown));
        row.append(&dropdown);
        let note = gtk4::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_visible(false);
        note.add_css_class("dim-label");
        let this = Rc::new(CursorSize {
            row,
            note,
            model,
            dropdown,
            sizes: RefCell::new(Vec::new()),
            updating: Cell::new(false),
            applying: Cell::new(false),
        });
        let weak = Rc::downgrade(&this);
        this.dropdown.connect_selected_notify(move |_| {
            if let Some(me) = weak.upgrade() {
                me.on_selected();
            }
        });
        this
    }

    /// Os tamanhos do tema instalado, com o do config selecionado (ou o mais perto, o que o Xcursor mostra). Enquanto
    /// aplica, nada: o fim da aplicação recarrega com o tamanho que ficou.
    pub fn load(&self, config: &Config) {
        if self.applying.get() {
            return;
        }
        let sizes = theme_sizes();
        let current = closest_size(&sizes, config.cursor_size);
        self.updating.set(true);
        let labels: Vec<String> = sizes.iter().map(|size| fill_one(&tr("%s px"), &size.to_string())).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.model.splice(0, self.model.n_items(), &labels);
        if let Some(index) = sizes.iter().position(|size| *size == current) {
            self.dropdown.set_selected(index as u32);
        }
        *self.sizes.borrow_mut() = sizes;
        self.updating.set(false);
    }

    fn on_selected(self: &Rc<Self>) {
        if self.updating.get() || self.applying.get() {
            return;
        }
        let Some(size) = self.sizes.borrow().get(self.dropdown.selected() as usize).copied() else { return };
        self.applying.set(true);
        self.dropdown.set_sensitive(false);
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || message(st::locked(|| st::apply_cursor_size(size)).and_then(|r| r))).await;
            let Some(me) = me.upgrade() else { return };
            let (_, text) = result.unwrap_or_else(|_| (false, fill_one(&tr("Could not update: %s"), &tr("Cursor size"))));
            me.note.set_text(&text);
            me.note.set_visible(!text.is_empty());
            me.applying.set(false);
            // A lista volta ao tamanho do config: o aplicado ou, se falhou, o de antes. Um config que não se lê daria o
            // padrão, que não é o de ninguém: aí a lista fica como está
            let (config, problems) = load_config(&config::path());
            if !problems.iter().any(|p| p.contains("could not read")) {
                me.load(&config);
            }
            me.dropdown.set_sensitive(true);
        });
    }
}
