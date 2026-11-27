//! O painel Energia e bloqueio: os tempos sem uso do sway-idle (lock_after, screen_off_after, suspend_after, em
//! minutos; 0 é nunca) e o perfil de energia do power-profiles-daemon. Os tempos são chaves soltas do config.toml
//! do usuário, gravadas pelo config_edit; o sway-idle (do swaydesk-caffeine) os lê e é rodado de novo para valerem
//! na hora, só numa sessão do Swaydesk (é ela quem o abre). O perfil vai direto ao daemon, como no popup da
//! bateria. As regras são do swaydesk-settings-logic (power, layers).
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::config::{self, IDLE_KEYS};
use swaydesk_common::config_edit;
use swaydesk_common::i18n::tr;
use swaydesk_common::{paths, power};
use swaydesk_settings_logic::desktop::Serial;
use swaydesk_settings_logic::layers::Layers;
use swaydesk_settings_logic::power::{choices, edit_for, screen_off_before_lock, PACKAGE, PROGRAM};

use crate::desktop::{heading, heading_with_note, load, read, run_ok, with};
use crate::region::line;

/// Os nomes dos perfis, os do popup da bateria.
fn profile_name(key: &str) -> String {
    match key {
        "power-saver" => tr("Power saver"),
        "balanced" => tr("Balanced"),
        "performance" => tr("Performance"),
        other => other.to_string(),
    }
}

fn minutes_name(minutes: u32) -> String {
    if minutes == 0 {
        tr("Never")
    } else {
        with(tr("%(minutes)s min"), "minutes", &minutes.to_string())
    }
}

/// O sway-idle que vale rodar de novo: só numa sessão do Swaydesk, que o abriu (num Sway puro, o swayidle é o do
/// usuário, e o sway-idle o trocaria).
fn running_idle() -> Option<PathBuf> {
    paths::in_session_loop(PROGRAM)
}

struct Idle {
    key: &'static str,
    dropdown: gtk4::DropDown,
    /// Os minutos de cada item da lista, na ordem dela
    values: RefCell<Vec<u32>>,
}

struct Panel {
    layers: RefCell<Layers>,
    /// Os minutos em vigor, na ordem dos IDLE_KEYS (os padrões sem a chave)
    minutes: RefCell<Vec<u32>>,
    message: gtk4::Label,
    hint: gtk4::Label,
    idles: Vec<Idle>,
    filling: Cell<bool>,
    serial: RefCell<Serial>,
}

impl Panel {
    fn show_error(&self, text: &str) {
        self.message.set_text(text);
        self.message.set_visible(true);
    }

    fn reload(self: &Rc<Self>) {
        *self.layers.borrow_mut() = load();
        let text = read(&config::path()).unwrap_or_default();
        let parsed = config::parse_config(&text).0;
        *self.minutes.borrow_mut() = IDLE_KEYS.iter().map(|k| parsed.idle_minutes(k).unwrap_or(0)).collect();
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
        let minutes = self.minutes.borrow();
        for (idle, current) in self.idles.iter().zip(minutes.iter()) {
            let values = choices(*current);
            // O modelo só muda se a lista mudar: trocá-lo dentro do selected-notify deixa a lista aberta
            if *idle.values.borrow() != values {
                let names: Vec<String> = values.iter().map(|m| minutes_name(*m)).collect();
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                idle.dropdown.set_model(Some(&gtk4::StringList::new(&refs)));
            }
            let selected = values.iter().position(|m| m == current).unwrap_or(0) as u32;
            if idle.dropdown.selected() != selected {
                idle.dropdown.set_selected(selected);
            }
            *idle.values.borrow_mut() = values;
            idle.dropdown.set_sensitive(editable);
        }
        self.hint.set_visible(screen_off_before_lock(minutes[0], minutes[1]));
        self.filling.set(false);
    }

    fn save(self: &Rc<Self>, key: &str, minutes: u32) {
        let edit = match edit_for(key, minutes) {
            Ok(edit) => edit,
            Err(err) => return self.show_error(&with(tr("Not saved:\n%(error)s"), "error", &err)),
        };
        if let Err(err) = config_edit::write(&config::path(), &[edit]) {
            self.reload();
            if self.layers.borrow().user_error.is_none() {
                self.show_error(&with(tr("Could not save:\n%(error)s"), "error", &err));
            }
            return;
        }
        self.reload();
        if running_idle().is_some() && self.serial.borrow_mut().request() {
            self.spawn_idle();
        }
    }

    /// Roda o sway-idle de novo (ele troca o swayidle em execução), um por vez: uma escolha durante a troca
    /// pede mais uma, que lê o arquivo já gravado.
    fn spawn_idle(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let ok = gio::spawn_blocking(|| running_idle().is_some_and(|program| run_ok(&program, &[])))
                .await
                .unwrap_or(false);
            let Some(panel) = weak.upgrade() else { return };
            if !ok {
                panel.show_error(&tr("The times were saved, but not applied"));
            }
            if panel.serial.borrow_mut().finished() {
                panel.spawn_idle();
            }
        });
    }
}

fn list() -> gtk4::ListBox {
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

/// O perfil de energia, só com o power-profiles-daemon no ar (a lista dele vem vazia sem ele).
/// O perfil de energia, só com o power-profiles-daemon no ar (a lista dele vem vazia sem ele). As duas perguntas ao
/// daemon (até 3 s cada, pelo busctl) vão numa thread: a seção entra na página quando elas voltam, e a janela não
/// espera um daemon lento ou que ainda está subindo.
fn profiles_section(content: &gtk4::Box, message: &gtk4::Label) {
    let section = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    section.set_visible(false);
    content.append(&section);
    let message = message.clone();
    glib::spawn_future_local(async move {
        let Ok((profiles, active)) = gio::spawn_blocking(|| (power::get_profiles(), power::get_profile())).await else {
            return;
        };
        if profiles.is_empty() {
            return;
        }
        let names: Vec<String> = profiles.iter().map(|p| profile_name(p)).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let dropdown = gtk4::DropDown::from_strings(&refs);
        dropdown.set_selected(profiles.iter().position(|p| *p == active).unwrap_or(0) as u32);
        dropdown.connect_selected_notify(move |dropdown| {
            let Some(key) = profiles.get(dropdown.selected() as usize).cloned() else { return };
            let message = message.clone();
            glib::spawn_future_local(async move {
                let ok = gio::spawn_blocking(move || power::set_profile(&key)).await.unwrap_or(false);
                if !ok {
                    message.set_text(&tr("The power profile did not change"));
                    message.set_visible(true);
                }
            });
        });
        section.append(&heading(&tr("Power profile"), &tr("Changes apply at once")));
        let profiles_list = list();
        append(&profiles_list, &line(&tr("Profile"), &tr("Less power, or more speed"), &dropdown));
        section.append(&profiles_list);
        section.set_visible(true);
    });
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

    // O aviso de quando vale e o "não instalado" são refeitos a cada vez que a página aparece (show_state): o
    // swaydesk-caffeine pode ter sido instalado com a janela aberta
    let (header, note) = heading_with_note(&tr("When idle"), "");
    content.append(&header);

    let hint = gtk4::Label::new(Some(&tr("The screen goes off before it locks")));
    hint.add_css_class("dim-label");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.set_visible(false);

    let panel = Rc::new(Panel {
        layers: RefCell::new(Layers::default()),
        minutes: RefCell::new(vec![0; IDLE_KEYS.len()]),
        message: message.clone(),
        hint: hint.clone(),
        idles: IDLE_KEYS
            .iter()
            .map(|key| Idle { key, dropdown: gtk4::DropDown::from_strings(&[]), values: RefCell::new(Vec::new()) })
            .collect(),
        filling: Cell::new(false),
        serial: RefCell::default(),
    });

    let idle_list = list();
    {
        let labels = [
            (tr("Lock the screen after"), String::new()),
            (tr("Turn the screen off after"), String::new()),
            (tr("Suspend after"), tr("Only on battery. The screen always locks before suspending")),
        ];
        for (idle, (label, row_note)) in panel.idles.iter().zip(labels) {
            append(&idle_list, &line(&label, &row_note, &idle.dropdown));
            let weak = Rc::downgrade(&panel);
            let key = idle.key;
            idle.dropdown.connect_selected_notify(move |dropdown| {
                let Some(panel) = weak.upgrade().filter(|p| !p.filling.get()) else { return };
                let idle = panel.idles.iter().find(|i| i.key == key).expect("an idle row");
                let minutes = idle.values.borrow().get(dropdown.selected() as usize).copied();
                if let Some(minutes) = minutes {
                    panel.save(key, minutes);
                }
            });
        }
        content.append(&idle_list);
        content.append(&hint);
    }
    let missing = gtk4::Label::new(Some(&with(tr("Not installed (%(package)s)"), "package", PACKAGE)));
    missing.add_css_class("dim-label");
    missing.set_xalign(0.0);
    content.append(&missing);

    profiles_section(&content, &message);

    panel.reload();
    // Sem o sway-idle (o swaydesk-caffeine), a seção diz o que falta, em vez de listas que não fazem nada
    let hint_shown = hint.clone();
    let show_state = move || {
        let running = running_idle().is_some();
        let installed = running || paths::find_helper(PROGRAM).is_some();
        idle_list.set_visible(installed);
        missing.set_visible(!installed);
        note.set_text(&if !installed {
            String::new()
        } else if running {
            tr("Changes apply at once. With the caffeine on, none of them applies")
        } else {
            tr("These times apply in the Swaydesk session")
        });
        note.set_visible(installed);
        if !installed {
            hint_shown.set_visible(false);
        }
    };
    show_state();
    let scroll = gtk4::ScrolledWindow::new();
    scroll.connect_map(move |_| show_state());
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&content));
    // O painel vive enquanto a página viver: o tratador do destroy é dono dele (os outros só têm o Weak)
    scroll.connect_destroy(move |_| {
        let _ = &panel;
    });
    scroll.upcast()
}
