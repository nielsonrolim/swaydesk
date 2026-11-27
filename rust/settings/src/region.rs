//! O painel Região: o idioma do Swaydesk (locale), a cidade da previsão do tempo (weather_location) e o país dos
//! feriados (holidays). São chaves soltas do config.toml do usuário (a camada do sistema não as tem), gravadas
//! pelo config_edit; um campo vazio tira a chave e vale o padrão. As regras são do swaydesk-settings-logic
//! (region, layers).
use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use swaydesk_common::config;
use swaydesk_common::config_edit;
use swaydesk_common::i18n::tr;
use swaydesk_settings_logic::layers::Layers;
use swaydesk_settings_logic::region::{edit_for, missing_locale, LANGUAGES};

use crate::desktop::{load, with};

struct Panel {
    layers: RefCell<Layers>,
    message: gtk4::Label,
    language: gtk4::DropDown,
    /// Os códigos da lista do idioma, na ordem dela: os LANGUAGES e, se o config tiver outro, ele no fim
    codes: RefCell<Vec<String>>,
    city: gtk4::Entry,
    holidays: gtk4::Entry,
    /// O aviso de quando o locale do idioma escolhido não está gerado (a data fica na língua do sistema)
    date_note: gtk4::Label,
    filling: std::cell::Cell<bool>,
}

/// Se o locale existe no sistema (gerado, no Debian e no Arch): o mesmo teste que o setlocale do relógio faria.
fn locale_generated(name: &str) -> bool {
    let Ok(name) = std::ffi::CString::new(name) else { return false };
    // SAFETY: o nome é uma string C válida; o locale devolvido, se houver, é liberado na hora
    unsafe {
        let locale = libc::newlocale(libc::LC_TIME_MASK, name.as_ptr(), std::ptr::null_mut());
        if locale.is_null() {
            return false;
        }
        libc::freelocale(locale);
    }
    true
}

impl Panel {
    fn show_error(&self, text: &str) {
        self.message.set_text(text);
        self.message.set_visible(true);
    }

    /// Grava o texto do campo; um valor que o config não aceita fica no campo, com o erro.
    fn save(self: &Rc<Self>, key: &str, text: &str) {
        let edit = match edit_for(key, text) {
            Ok(edit) => edit,
            Err(err) => {
                self.show_error(&with(tr("Not saved:\n%(error)s"), "error", &err));
                return;
            }
        };
        if let Err(err) = config_edit::write(&config::path(), &[edit]) {
            self.reload();
            if self.layers.borrow().user_error.is_none() {
                self.show_error(&with(tr("Could not save:\n%(error)s"), "error", &err));
            }
            return;
        }
        self.reload();
    }

    fn reload(self: &Rc<Self>) {
        *self.layers.borrow_mut() = load();
        self.fill();
    }

    fn fill(&self) {
        self.filling.set(true);
        let layers = self.layers.borrow();
        let editable = layers.user_error.is_none();
        match &layers.user_error {
            Some(err) => {
                let first = err.lines().next().unwrap_or_default();
                self.show_error(&with(tr("Your config.toml has an error. Fix it to change these settings:\n%(error)s"), "error", first));
            }
            None => self.message.set_visible(false),
        }
        // O idioma: um dos LANGUAGES, ou o do config no fim da lista (um locale válido que não tem catálogo)
        let locale = layers.value("locale").unwrap_or_default();
        let mut codes: Vec<String> = LANGUAGES.iter().map(|(code, _)| code.to_string()).collect();
        let mut names: Vec<String> =
            LANGUAGES.iter().enumerate().map(|(i, (_, name))| if i == 0 { tr(name) } else { name.to_string() }).collect();
        if !codes.contains(&locale) {
            codes.push(locale.clone());
            names.push(locale.clone());
        }
        // O modelo só muda se a lista mudar: trocá-lo dentro do selected-notify (a gravação recarrega a página)
        // deixava a lista aberta, com a marca no item antigo
        if *self.codes.borrow() != codes {
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            self.language.set_model(Some(&gtk4::StringList::new(&refs)));
        }
        let selected = codes.iter().position(|c| *c == locale).unwrap_or(0) as u32;
        if self.language.selected() != selected {
            self.language.set_selected(selected);
        }
        *self.codes.borrow_mut() = codes;
        match missing_locale(&locale, locale_generated) {
            Some(name) => {
                self.date_note.set_text(&with(
                    tr("The texts change at the next login, but the date stays in the system's language until the %(locale)s locale is generated on this computer"),
                    "locale",
                    &name,
                ));
                self.date_note.set_visible(true);
            }
            None => self.date_note.set_visible(false),
        }
        self.city.set_text(&layers.value("weather_location").unwrap_or_default());
        self.holidays.set_text(&layers.value("holidays").unwrap_or_default());
        for widget in [self.language.upcast_ref::<gtk4::Widget>(), self.city.upcast_ref(), self.holidays.upcast_ref()] {
            widget.set_sensitive(editable);
        }
        self.filling.set(false);
    }
}

/// Uma linha: o nome e o aviso à esquerda, o controle à direita.
pub(crate) fn line(label: &str, note: &str, control: &impl IsA<gtk4::Widget>) -> gtk4::Box {
    let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    line.set_margin_top(8);
    line.set_margin_bottom(8);
    line.set_margin_start(12);
    line.set_margin_end(12);
    let texts = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    texts.set_valign(gtk4::Align::Center);
    let name = gtk4::Label::new(Some(label));
    name.set_xalign(0.0);
    let note = gtk4::Label::new(Some(note));
    note.add_css_class("dim-label");
    note.add_css_class("caption");
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.set_visible(!note.text().is_empty());
    texts.append(&name);
    texts.append(&note);
    line.append(&texts);
    control.set_valign(gtk4::Align::Center);
    line.append(control);
    line
}

/// Um campo de texto que grava ao apertar Enter ou ao sair dele.
fn entry(placeholder: &str) -> gtk4::Entry {
    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some(placeholder));
    entry.set_width_chars(18);
    entry
}

fn connect_save(panel: &Rc<Panel>, entry: &gtk4::Entry, key: &'static str) {
    let weak = Rc::downgrade(panel);
    entry.connect_activate(move |entry| {
        if let Some(panel) = weak.upgrade() {
            panel.save(key, &entry.text());
        }
    });
    // Ao sair do campo, só se mudou: a página remontada não grava de novo
    let weak = Rc::downgrade(panel);
    let focus = gtk4::EventControllerFocus::new();
    let field = entry.downgrade();
    focus.connect_leave(move |_| {
        let (Some(panel), Some(entry)) = (weak.upgrade(), field.upgrade()) else { return };
        if !panel.filling.get() && entry.text().trim() != panel.layers.borrow().value(key).unwrap_or_default() {
            panel.save(key, &entry.text());
        }
    });
    entry.add_controller(focus);
}

pub fn page() -> gtk4::Widget {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_halign(gtk4::Align::Center);
    content.set_width_request(560);

    let message = gtk4::Label::new(None);
    message.add_css_class("error");
    message.set_wrap(true);
    message.set_xalign(0.0);
    message.set_visible(false);
    content.append(&message);

    let panel = Rc::new(Panel {
        layers: RefCell::new(Layers::default()),
        message,
        language: gtk4::DropDown::from_strings(&[]),
        codes: RefCell::new(Vec::new()),
        city: entry(&tr("From your location")),
        holidays: entry(&tr("The language's country")),
        date_note: gtk4::Label::new(None),
        filling: std::cell::Cell::new(false),
    });
    panel.date_note.add_css_class("dim-label");
    panel.date_note.set_xalign(0.0);
    panel.date_note.set_wrap(true);
    // Sem um máximo, a largura natural do aviso é a da frase inteira, e a página alarga (os campos saem do lugar)
    panel.date_note.set_max_width_chars(60);
    panel.date_note.set_visible(false);

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    for row in [
        line(&tr("Language"), &tr("Changes apply at the next login"), &panel.language),
        line(&tr("Weather city"), &tr("Empty: the city of your connection. The bar's weather updates within a minute"), &panel.city),
        line(&tr("Holidays"), &tr("A country code such as BR, or none. Empty: the language's country"), &panel.holidays),
    ] {
        let item = gtk4::ListBoxRow::new();
        item.set_activatable(false);
        item.set_child(Some(&row));
        list.append(&item);
    }
    content.append(&list);
    content.append(&panel.date_note);

    let weak = Rc::downgrade(&panel);
    panel.language.connect_selected_notify(move |dropdown| {
        let Some(panel) = weak.upgrade().filter(|p| !p.filling.get()) else { return };
        let code = panel.codes.borrow().get(dropdown.selected() as usize).cloned().unwrap_or_default();
        panel.save("locale", &code);
    });
    connect_save(&panel, &panel.city, "weather_location");
    connect_save(&panel, &panel.holidays, "holidays");

    panel.reload();
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&content));
    // O painel vive enquanto a página viver: o tratador do destroy é dono dele (os outros só têm o Weak)
    scroll.connect_destroy(move |_| {
        let _ = &panel;
    });
    scroll.upcast()
}
