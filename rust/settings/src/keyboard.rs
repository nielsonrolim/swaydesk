//! O painel Teclado: os layouts e a ordem deles, a tecla que troca de layout, a repetição das teclas e o Num Lock
//! no login. Grava só o que se muda aqui no 80-input.conf (input.rs) e aplica ao vivo; "Usar o teclado do
//! sistema" tira os layouts e as opções do arquivo, e o bloco do 50-local.conf (o teclado do sistema, que o login
//! acompanha) volta a valer. A tela de login sempre usa o teclado do sistema. As regras são do
//! swaydesk-settings-logic (input, keyboard).
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::i18n::{layout_name, tr};
use swaydesk_settings_logic::input::{system_layouts, Device, InputConf, Layout, Setting};
use swaydesk_settings_logic::keyboard::{catalog, switch_of, with_switch, Choice, Switch, REGISTRIES, SWITCHES};

use crate::desktop::{heading, with};
use crate::input::{effective, notes, ours, snapshot, Applier, Snapshot};
use crate::region::line;

const LAYOUT_KEYS: [(Device, &str); 1] = [(Device::Keyboard, "xkb_layout")];
const OPTION_KEYS: [(Device, &str); 1] = [(Device::Keyboard, "xkb_options")];

fn switch_name(switch: Switch, options: &[String]) -> String {
    match switch {
        Switch::Shortcut => tr("Only the shortcut"),
        Switch::AltShift => "Alt+Shift".into(),
        Switch::CtrlShift => "Ctrl+Shift".into(),
        Switch::CapsLock => tr("Caps Lock"),
        Switch::Other => {
            let option = options.iter().find(|o| o.starts_with("grp:")).cloned().unwrap_or_default();
            with(tr("Other (%(option)s)"), "option", &option)
        }
    }
}

/// Os layouts do registro com o nome na língua da sessão, na ordem dela (o strcoll: o GTK já chamou o setlocale),
/// e o texto que a busca procura em cada um: o nome traduzido, o nome em inglês e o código ("br abnt2").
fn translated_catalog() -> (Vec<Choice>, Vec<String>) {
    let xml = REGISTRIES.iter().find_map(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
    let mut list: Vec<(std::ffi::CString, Choice, String)> = catalog(&xml)
        .into_iter()
        .filter_map(|mut c| {
            let english = std::mem::take(&mut c.description);
            c.description = layout_name(&english);
            let search = format!("{} {english} {} {}", c.description, c.layout.layout, c.layout.variant).to_lowercase();
            Some((std::ffi::CString::new(c.description.clone()).ok()?, c, search))
        })
        .collect();
    // SAFETY: os dois são textos C terminados em zero, vivos durante a chamada
    list.sort_by(|a, b| unsafe { libc::strcoll(a.0.as_ptr(), b.0.as_ptr()) }.cmp(&0));
    list.into_iter().map(|(_, c, search)| (c, search)).unzip()
}

struct Panel {
    catalog: Vec<Choice>,
    /// O texto da busca de cada item do catálogo
    search: Vec<String>,
    applier: Rc<Applier>,
    snapshot: RefCell<Snapshot>,
    source: gtk4::Label,
    layouts: gtk4::ListBox,
    system: gtk4::Button,
    switch: gtk4::DropDown,
    switches: RefCell<Vec<Switch>>,
    options: RefCell<Vec<String>>,
    delay: gtk4::SpinButton,
    rate: gtk4::SpinButton,
    numlock: gtk4::Switch,
    hint: gtk4::Label,
    filling: Cell<bool>,
    /// A espera de cada campo numérico (o atraso, a velocidade): grava quando ele para de mudar
    pending_spin: [RefCell<Option<glib::SourceId>>; 2],
}

impl Panel {
    fn name_of(&self, layout: &Layout) -> String {
        self.catalog
            .iter()
            .find(|c| c.layout == *layout)
            .map(|c| c.description.clone())
            .unwrap_or_else(|| match layout.variant.as_str() {
                "" => layout.layout.clone(),
                variant => format!("{} ({variant})", layout.layout),
            })
    }

    /// Lê o Sway fora da thread do GTK e preenche.
    fn refresh(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let snapshot = gio::spawn_blocking(snapshot).await.unwrap_or_default();
            let Some(panel) = weak.upgrade() else { return };
            *panel.snapshot.borrow_mut() = snapshot;
            panel.fill();
        });
    }

    fn current(&self) -> (InputConf, InputConf) {
        (effective(self.snapshot.borrow().main_config.as_deref()), ours())
    }

    fn fill(self: &Rc<Self>) {
        self.filling.set(true);
        let (now, mine) = self.current();
        let find = |conf: &InputConf, key: &str| conf.keyboard.iter().find(|s| s.key() == key).cloned();
        let layouts = system_layouts(&now, |name| std::env::var(name).ok());
        let chosen = find(&mine, "xkb_layout").is_some() || find(&mine, "xkb_options").is_some();
        self.source.set_text(&if chosen { tr("Chosen here") } else { tr("The system keyboard") });
        self.system.set_visible(chosen);

        while let Some(row) = self.layouts.first_child() {
            self.layouts.remove(&row);
        }
        for (i, layout) in layouts.iter().enumerate() {
            self.layouts.append(&self.layout_row(&layouts, i, &self.name_of(layout)));
        }

        let options = match find(&now, "xkb_options") {
            Some(Setting::Options(list)) => list,
            _ => Vec::new(),
        };
        let current = switch_of(&options);
        let mut switches = SWITCHES.to_vec();
        if current == Switch::Other {
            switches.push(Switch::Other);
        }
        if *self.switches.borrow() != switches || *self.options.borrow() != options {
            let names: Vec<String> = switches.iter().map(|s| switch_name(*s, &options)).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            self.switch.set_model(Some(&gtk4::StringList::new(&refs)));
        }
        let selected = switches.iter().position(|s| *s == current).unwrap_or(0) as u32;
        if self.switch.selected() != selected {
            self.switch.set_selected(selected);
        }
        *self.switches.borrow_mut() = switches;
        *self.options.borrow_mut() = options;

        if let Some(Setting::RepeatDelay(ms)) = find(&now, "repeat_delay") {
            self.delay.set_value(f64::from(ms));
        } else {
            self.delay.set_value(600.0);
        }
        if let Some(Setting::RepeatRate(rate)) = find(&now, "repeat_rate") {
            self.rate.set_value(f64::from(rate));
        } else {
            self.rate.set_value(25.0);
        }
        self.numlock.set_active(matches!(find(&now, "xkb_numlock"), Some(Setting::NumLock(true))));

        let hint = notes(&self.snapshot.borrow(), &[Device::Keyboard]);
        self.hint.set_visible(hint.is_some());
        self.hint.set_text(&hint.unwrap_or_default());
        self.filling.set(false);
    }

    /// Uma linha da lista: o nome e os botões subir, descer e tirar.
    fn layout_row(self: &Rc<Self>, layouts: &[Layout], index: usize, name: &str) -> gtk4::ListBoxRow {
        let row = gtk4::ListBoxRow::new();
        row.set_activatable(false);
        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        content.set_margin_top(6);
        content.set_margin_bottom(6);
        content.set_margin_start(12);
        content.set_margin_end(6);
        let label = gtk4::Label::new(Some(name));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        content.append(&label);
        let buttons: [(&str, String, bool, isize); 3] = [
            ("go-up-symbolic", tr("Move up"), index > 0, -1),
            ("go-down-symbolic", tr("Move down"), index + 1 < layouts.len(), 1),
            ("list-remove-symbolic", tr("Remove"), layouts.len() > 1, 0),
        ];
        for (icon, tooltip, sensitive, step) in buttons {
            let button = swaydesk_ui::icon_button(icon, &tooltip);
            button.add_css_class("flat");
            button.set_sensitive(sensitive);
            let weak = Rc::downgrade(self);
            let list = layouts.to_vec();
            button.connect_clicked(move |_| {
                let Some(panel) = weak.upgrade() else { return };
                let mut list = list.clone();
                if step == 0 {
                    list.remove(index);
                } else {
                    list.swap(index, index.saturating_add_signed(step));
                }
                panel.set_layouts(list);
            });
            content.append(&button);
        }
        row.set_child(Some(&content));
        row
    }

    fn set_layouts(self: &Rc<Self>, list: Vec<Layout>) {
        self.applier.change(&LAYOUT_KEYS, |conf| conf.set(Device::Keyboard, Setting::Layouts(list)));
        self.refresh();
    }

    fn add_layout(self: &Rc<Self>, layout: Layout) {
        let (now, _) = self.current();
        // Parte do teclado que vale, também o do XKB_DEFAULT_LAYOUT: adicionar não troca o do sistema pelo us
        let mut list = system_layouts(&now, |name| std::env::var(name).ok());
        if !list.contains(&layout) {
            list.push(layout);
            self.set_layouts(list);
        }
    }

    fn use_system(self: &Rc<Self>) {
        let keys = [LAYOUT_KEYS[0], OPTION_KEYS[0]];
        self.applier.change(&keys, |conf| {
            conf.unset(Device::Keyboard, &Setting::Layouts(Vec::new()));
            conf.unset(Device::Keyboard, &Setting::Options(Vec::new()));
        });
        self.refresh();
    }

    fn set(self: &Rc<Self>, key: &'static str, setting: Setting) {
        if self.filling.get() {
            return;
        }
        self.applier.change(&[(Device::Keyboard, key)], |conf| conf.set(Device::Keyboard, setting));
        self.refresh();
    }
}

/// O botão "Adicionar um layout": a lista do registro, com busca.
fn add_button(panel: &Rc<Panel>) -> gtk4::MenuButton {
    let search = swaydesk_ui::search_entry(false, swaydesk_ui::SearchSize::Regular);
    let list = gtk4::ListBox::new();
    list.set_activate_on_single_click(true);
    for choice in &panel.catalog {
        let label = gtk4::Label::new(Some(&choice.description));
        label.set_xalign(0.0);
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        label.set_margin_start(8);
        list.append(&label);
    }
    let query = Rc::new(RefCell::new(String::new()));
    {
        let query = query.clone();
        let search = panel.search.clone();
        list.set_filter_func(move |row| {
            search.get(row.index() as usize).is_some_and(|text| text.contains(query.borrow().as_str()))
        });
    }
    {
        let list = list.clone();
        search.connect_search_changed(move |entry| {
            *query.borrow_mut() = entry.text().to_lowercase();
            list.invalidate_filter();
        });
    }
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_min_content_height(320);
    scroll.set_min_content_width(360);
    scroll.set_child(Some(&list));
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    content.append(&search);
    content.append(&scroll);
    let popover = gtk4::Popover::new();
    popover.set_child(Some(&content));
    let button = gtk4::MenuButton::new();
    button.set_label(&tr("Add a layout…"));
    button.set_popover(Some(&popover));
    let weak = Rc::downgrade(panel);
    list.connect_row_activated(move |_, row| {
        let Some(panel) = weak.upgrade() else { return };
        let Some(choice) = panel.catalog.get(row.index() as usize) else { return };
        popover.popdown();
        panel.add_layout(choice.layout.clone());
    });
    button
}

fn boxed_list() -> gtk4::ListBox {
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    list
}

fn append(list: &gtk4::ListBox, child: &impl IsA<gtk4::Widget>) {
    let item = gtk4::ListBoxRow::new();
    item.set_activatable(false);
    item.set_child(Some(child));
    list.append(&item);
}

pub fn page() -> gtk4::Widget {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_halign(gtk4::Align::Center);
    content.set_width_request(560);

    let applier = Applier::new();
    let hint = gtk4::Label::new(None);
    hint.add_css_class("dim-label");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    // Um identificador longo, sem espaços, quebra no meio em vez de alargar a página
    hint.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    hint.set_max_width_chars(70);
    hint.set_selectable(true);
    hint.set_visible(false);

    let (catalog, search) = translated_catalog();
    let panel = Rc::new(Panel {
        catalog,
        search,
        applier: applier.clone(),
        snapshot: RefCell::default(),
        source: gtk4::Label::new(None),
        layouts: boxed_list(),
        system: gtk4::Button::with_label(&tr("Use the system keyboard")),
        switch: gtk4::DropDown::from_strings(&[]),
        switches: RefCell::default(),
        options: RefCell::default(),
        delay: gtk4::SpinButton::with_range(100.0, 2000.0, 50.0),
        rate: gtk4::SpinButton::with_range(1.0, 100.0, 1.0),
        numlock: gtk4::Switch::new(),
        hint: hint.clone(),
        filling: Cell::new(false),
        pending_spin: Default::default(),
    });

    content.append(&heading(&tr("Keyboard layouts"), &tr("The login screen keeps the system keyboard")));
    let source_line = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    panel.source.add_css_class("dim-label");
    panel.source.set_xalign(0.0);
    panel.source.set_hexpand(true);
    source_line.append(&panel.source);
    source_line.append(&panel.system);
    content.append(&source_line);
    content.append(&panel.layouts);
    let add = add_button(&panel);
    add.set_halign(gtk4::Align::Start);
    content.append(&add);

    content.append(&heading(&tr("Switching layouts"), ""));
    let switching = boxed_list();
    let note = tr("Besides the layout shortcut, which is in the Help");
    append(&switching, &line(&tr("Switch key"), &note, &panel.switch));
    content.append(&switching);

    content.append(&heading(&tr("Typing"), ""));
    let typing = boxed_list();
    append(&typing, &line(&tr("Repeat delay"), &tr("Milliseconds before a held key repeats"), &panel.delay));
    append(&typing, &line(&tr("Repeat rate"), &tr("Repeats per second"), &panel.rate));
    append(&typing, &line(&tr("Num Lock on"), &tr("When the keyboard appears, as at login"), &panel.numlock));
    content.append(&typing);
    content.append(&hint);
    // Embaixo: uma mensagem que aparece não empurra os controles
    content.append(&panel.applier.status);

    {
        let weak = Rc::downgrade(&panel);
        panel.system.connect_clicked(move |_| {
            if let Some(panel) = weak.upgrade() {
                panel.use_system();
            }
        });
    }
    {
        let weak = Rc::downgrade(&panel);
        panel.switch.connect_selected_notify(move |dropdown| {
            let Some(panel) = weak.upgrade().filter(|p| !p.filling.get()) else { return };
            let Some(switch) = panel.switches.borrow().get(dropdown.selected() as usize).copied() else { return };
            let options = with_switch(&panel.options.borrow(), switch);
            panel.applier.change(&OPTION_KEYS, |conf| conf.set(Device::Keyboard, Setting::Options(options)));
            panel.refresh();
        });
    }
    let spins = [(panel.delay.clone(), "repeat_delay"), (panel.rate.clone(), "repeat_rate")];
    for (i, (spin, key)) in spins.into_iter().enumerate() {
        let weak = Rc::downgrade(&panel);
        spin.connect_value_changed(move |spin| {
            let Some(panel) = weak.upgrade().filter(|p| !p.filling.get()) else { return };
            if let Some(source) = panel.pending_spin[i].borrow_mut().take() {
                source.remove();
            }
            let value = spin.value_as_int().max(0) as u32;
            let weak = Rc::downgrade(&panel);
            let id = glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
                let Some(panel) = weak.upgrade() else { return };
                panel.pending_spin[i].borrow_mut().take();
                let setting =
                    if key == "repeat_delay" { Setting::RepeatDelay(value) } else { Setting::RepeatRate(value) };
                panel.set(key, setting);
            });
            *panel.pending_spin[i].borrow_mut() = Some(id);
        });
    }
    {
        let weak = Rc::downgrade(&panel);
        panel.numlock.connect_active_notify(move |switch| {
            if let Some(panel) = weak.upgrade() {
                panel.set("xkb_numlock", Setting::NumLock(switch.is_active()));
            }
        });
    }

    let scroll = gtk4::ScrolledWindow::new();
    {
        // O Sway e os arquivos podem mudar com a janela aberta: lidos de novo a cada vez que a página aparece
        let panel = panel.clone();
        scroll.connect_map(move |_| panel.refresh());
    }
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&content));
    scroll.upcast()
}
