//! O painel Desktop: liga e desliga os componentes da sessão ([enabled]) e os módulos da barra ([enabled.bar]),
//! e escolhe o perfil de atalhos. Grava no config.toml do usuário pelo config_edit (os comentários e o resto
//! ficam); a camada do sistema (/etc/swaydesk/config.toml) aparece como "Definido pelo sistema", e o "Usar o
//! padrão" tira a chave do usuário para ela (ou o padrão) valer de novo. Os dados e as regras são do
//! swaydesk-settings-logic (layers, desktop).
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::config::{self, SHORTCUT_PROFILES};
use swaydesk_common::config_edit::{self, Edit, Value};
use swaydesk_common::i18n::tr;
use swaydesk_common::paths;
use swaydesk_settings_logic::desktop::{apply_for, Apply, Program, Serial, BAR_LABELS, SWITCHES};
use swaydesk_settings_logic::layers::{Layers, Source};

/// O texto (já traduzido) com o %(name)s trocado.
pub(crate) fn with(text: String, name: &str, value: &str) -> String {
    text.replace(&format!("%({name})s"), value)
}

/// O texto de um arquivo; sem ele, vazio. Outro erro de leitura vira o erro da camada.
pub(crate) fn read(path: &std::path::Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// As duas camadas, como estão agora no disco.
pub fn load() -> Layers {
    let user = read(&config::path());
    let system = read(&config::system_path());
    let mut layers = Layers::parse(user.as_deref().unwrap_or(""), system.as_deref().unwrap_or(""));
    if let Err(err) = user {
        layers.user_error = Some(err);
    }
    if let Err(err) = system {
        layers.system_error = Some(err);
    }
    layers
}

fn installed(program: Program) -> bool {
    match program {
        Program::Component(launcher, binary) => paths::component(launcher, binary).is_file(),
        Program::Helper(name) => paths::find_helper(name).is_some(),
    }
}

/// O sway-waybar do laço da sessão, que regenera o config pelo [enabled.bar] e reabre a Waybar.
fn waybar_restart() -> Option<PathBuf> {
    paths::in_session_loop("sway-waybar")
}

/// O pacote do desktop-config, que o painel nomeia quando ele falta
const KEYS_PACKAGE: &str = "swaydesk-theme-tool";

/// O desktop-config (do swaydesk-theme-tool), que grava o config.d/00-keys.conf do perfil de atalhos.
pub(crate) fn desktop_config() -> Option<PathBuf> {
    Some(paths::component("sway-desktop-config", "swaydesk-desktop-config")).filter(|p| p.is_file())
}

/// Roda e diz se saiu com 0 (numa thread: o restart da Waybar espera a bandeja).
pub(crate) fn run_ok(program: &std::path::Path, args: &[&str]) -> bool {
    let mut command = std::process::Command::new(program);
    command.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null());
    swaydesk_ui::cmd::child_env(&mut command).status().is_ok_and(|s| s.success())
}

/// Faz a mudança valer, quando dá para ser na hora; o texto de uma falha.
fn apply(how: Apply) -> Result<(), String> {
    match how {
        Apply::Waybar => match waybar_restart() {
            Some(program) => run_ok(&program, &["restart"]).then_some(()).ok_or_else(|| tr("The bar did not restart")),
            None => Ok(()),
        },
        Apply::Keys => match desktop_config() {
            Some(program) => {
                if !run_ok(&program, &["keys"]) {
                    return Err(tr("The shortcuts were not written"));
                }
                // Fora de uma sessão do Sway não há o que recarregar
                if std::env::var_os("SWAYSOCK").is_some() && !run_ok(std::path::Path::new("swaymsg"), &["reload"]) {
                    return Err(tr("Sway did not reload"));
                }
                Ok(())
            }
            None => Ok(()),
        },
        Apply::Now | Apply::NextLogin => Ok(()),
    }
}

/// O aviso de quando as mudanças de uma seção valem.
fn when_text(how: Apply) -> String {
    match how {
        Apply::Waybar if waybar_restart().is_some() => tr("Changes apply at once"),
        Apply::Waybar => tr("Changes apply when the bar restarts"),
        Apply::Keys if desktop_config().is_some() => tr("Changes apply at once"),
        Apply::Now => tr("Changes apply at once"),
        _ => tr("Changes apply at the next login"),
    }
}

struct Panel {
    layers: RefCell<Layers>,
    message: gtk4::Label,
    switches: gtk4::ListBox,
    bar: gtk4::ListBox,
    shortcuts: gtk4::DropDown,
    filling: std::cell::Cell<bool>,
    /// Uma aplicação por vez: a da barra (sway-waybar restart) e a dos atalhos (desktop-config keys + reload)
    waybar_serial: RefCell<Serial>,
    keys_serial: RefCell<Serial>,
}

pub(crate) fn heading(text: &str, note: &str) -> gtk4::Box {
    heading_with_note(text, note).0
}

/// O título de uma seção e o aviso dele, para quem troca o aviso depois (o que falta instalar).
pub(crate) fn heading_with_note(text: &str, note: &str) -> (gtk4::Box, gtk4::Label) {
    let header = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    let title = gtk4::Label::new(Some(text));
    title.add_css_class("heading");
    title.set_xalign(0.0);
    let note = gtk4::Label::new(Some(note));
    note.add_css_class("dim-label");
    note.set_xalign(0.0);
    header.append(&title);
    header.append(&note);
    (header, note)
}

fn list() -> gtk4::ListBox {
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    list
}

impl Panel {
    /// Grava a mudança, relê as camadas, remonta as linhas e a faz valer.
    fn save(self: &Rc<Self>, key: &str, edit: Edit) {
        if let Err(err) = config_edit::write(&config::path(), &[edit]) {
            // O interruptor já mudou na tela: a página volta ao que está no disco. Um config.toml que ficou
            // inválido mostra o erro dele (com a linha) e trava os interruptores; outro erro (sem permissão) é o
            // da gravação
            self.reload();
            if self.layers.borrow().user_error.is_none() {
                self.show_error(&with(tr("Could not save:\n%(error)s"), "error", &err));
            }
            return;
        }
        self.reload();
        self.run_apply(apply_for(key));
    }

    /// Aplica a mudança, uma por vez por tipo (o Serial): um clique durante um `sway-waybar restart` só pede mais
    /// uma rodada, que lê o arquivo já gravado.
    fn run_apply(self: &Rc<Self>, how: Apply) {
        // O Now e o NextLogin não rodam nada (o tempo e os feriados releem o arquivo; o resto, no login)
        if self.serial(how).is_some_and(|serial| serial.borrow_mut().request()) {
            self.spawn_apply(how);
        }
    }

    fn spawn_apply(self: &Rc<Self>, how: Apply) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || apply(how)).await.unwrap_or_else(|_| Err(tr("The change was saved, but not applied")));
            let Some(panel) = weak.upgrade() else { return };
            if let Err(err) = result {
                panel.show_error(&err);
            }
            if panel.serial(how).is_some_and(|serial| serial.borrow_mut().finished()) {
                panel.spawn_apply(how);
            }
        });
    }

    fn serial(&self, how: Apply) -> Option<&RefCell<Serial>> {
        match how {
            Apply::Waybar => Some(&self.waybar_serial),
            Apply::Keys => Some(&self.keys_serial),
            Apply::Now | Apply::NextLogin => None,
        }
    }

    fn show_error(&self, text: &str) {
        self.message.set_text(text);
        self.message.set_visible(true);
    }

    fn reload(self: &Rc<Self>) {
        *self.layers.borrow_mut() = load();
        self.fill();
    }

    /// Uma linha: o nome, o aviso (de onde vem, ou que não está instalado), o "Usar o padrão" e o interruptor.
    fn row(self: &Rc<Self>, key: &str, label: &str, missing: Option<&str>, editable: bool) -> gtk4::ListBoxRow {
        let available = missing.is_none();
        let state = self.layers.borrow().enabled(key);
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
        texts.append(&name);
        let note = if let Some(package) = missing {
            Some(with(tr("Not installed (%(package)s)"), "package", package))
        } else if state.source == Source::System {
            Some(tr("Set by the system"))
        } else {
            None
        };
        if let Some(note) = note {
            let note = gtk4::Label::new(Some(&note));
            note.add_css_class("dim-label");
            note.add_css_class("caption");
            note.set_xalign(0.0);
            texts.append(&note);
        }
        line.append(&texts);
        if state.source == Source::User && editable {
            let reset = swaydesk_ui::icon_button("edit-undo-symbolic", &tr("Use the default"));
            reset.add_css_class("flat");
            reset.set_valign(gtk4::Align::Center);
            let panel = Rc::downgrade(self);
            let key = key.to_string();
            reset.connect_clicked(move |_| {
                if let Some(panel) = panel.upgrade() {
                    panel.save(&key, Edit::Remove(format!("enabled.{key}")));
                }
            });
            line.append(&reset);
        }
        let switch = gtk4::Switch::new();
        switch.set_valign(gtk4::Align::Center);
        switch.set_active(state.on);
        switch.set_sensitive(available && editable);
        switch.update_property(&[gtk4::accessible::Property::Label(label)]);
        let panel = Rc::downgrade(self);
        let key = key.to_string();
        switch.connect_active_notify(move |switch| {
            if let Some(panel) = panel.upgrade().filter(|p| !p.filling.get()) {
                panel.save(&key, Edit::Set(format!("enabled.{key}"), Value::Bool(switch.is_active())));
            }
        });
        line.append(&switch);
        let row = gtk4::ListBoxRow::new();
        row.set_activatable(false);
        row.set_child(Some(&line));
        row
    }

    fn fill(self: &Rc<Self>) {
        self.filling.set(true);
        let (user_error, system_error) = {
            let layers = self.layers.borrow();
            (layers.user_error.clone(), layers.system_error.clone())
        };
        // Com o config.toml inválido, os interruptores mostrariam os padrões: o erro, e nada se grava
        let editable = user_error.is_none();
        // Do erro do toml, só a primeira linha ("TOML parse error at line 1, column 10"): o resto é o trecho do
        // arquivo. O erro vai numa linha própria da mensagem: um rótulo com uma linha longa alarga a página (o GTK
        // pede a largura dela inteira; nem o max-width-chars nem o natural-wrap-mode a seguraram)
        let first = |err: &String| err.lines().next().unwrap_or_default().to_string();
        let (user_error, system_error) = (user_error.as_ref().map(first), system_error.as_ref().map(first));
        match (&user_error, &system_error) {
            (Some(err), _) => self.show_error(&with(tr("Your config.toml has an error. Fix it to change these settings:\n%(error)s"), "error", err)),
            (None, Some(err)) => self.show_error(&with(tr("The system's config.toml has an error:\n%(error)s"), "error", err)),
            (None, None) => self.message.set_visible(false),
        }
        for list in [&self.switches, &self.bar] {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
        }
        for info in SWITCHES {
            let missing = (!installed(info.program)).then_some(info.package);
            self.switches.append(&self.row(info.key, &tr(info.label), missing, editable));
        }
        for (module, label) in BAR_LABELS {
            self.bar.append(&self.row(&format!("bar.{module}"), &tr(label), None, editable));
        }
        let current = self.layers.borrow().value("shortcuts").unwrap_or_else(|| "swaydesk".to_string());
        let index = SHORTCUT_PROFILES.iter().position(|(id, _)| *id == current).unwrap_or(0);
        self.shortcuts.set_selected(index as u32);
        self.shortcuts.set_sensitive(editable);
        self.filling.set(false);
    }
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
    message.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    message.set_xalign(0.0);
    message.set_visible(false);
    content.append(&message);

    let names: Vec<&str> = SHORTCUT_PROFILES.iter().map(|(_, name)| *name).collect();
    let panel = Rc::new(Panel {
        layers: RefCell::new(Layers::default()),
        message,
        switches: list(),
        bar: list(),
        shortcuts: gtk4::DropDown::from_strings(&names),
        filling: std::cell::Cell::new(false),
        waybar_serial: RefCell::default(),
        keys_serial: RefCell::default(),
    });

    content.append(&heading(&tr("Desktop components"), &when_text(Apply::NextLogin)));
    content.append(&panel.switches);
    content.append(&heading(&tr("Bar modules"), &when_text(Apply::Waybar)));
    content.append(&panel.bar);

    let (keys_heading, keys_note) = heading_with_note(&tr("Keyboard shortcuts"), &when_text(Apply::Keys));
    content.append(&keys_heading);
    let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let label = gtk4::Label::new(Some(&tr("Shortcut profile")));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    line.append(&label);
    line.append(&panel.shortcuts);
    content.append(&line);
    // Sem o desktop-config (do swaydesk-theme-tool), ninguém grava o 00-keys.conf do perfil, nem no login: a seção
    // diz o que falta, em vez de uma escolha que não vale. Refeito a cada vez que a página aparece (o pacote pode
    // ter sido instalado ou tirado com a janela aberta)
    let keys_missing = gtk4::Label::new(Some(&with(tr("Not installed (%(package)s)"), "package", KEYS_PACKAGE)));
    keys_missing.add_css_class("dim-label");
    keys_missing.set_xalign(0.0);
    content.append(&keys_missing);
    let keys_state = move || {
        let installed = desktop_config().is_some();
        line.set_visible(installed);
        keys_missing.set_visible(!installed);
        keys_note.set_text(&when_text(Apply::Keys));
        keys_note.set_visible(installed);
    };
    keys_state();
    let weak = Rc::downgrade(&panel);
    panel.shortcuts.connect_selected_notify(move |dropdown| {
        let Some(panel) = weak.upgrade().filter(|p| !p.filling.get()) else { return };
        if let Some((id, _)) = SHORTCUT_PROFILES.get(dropdown.selected() as usize) {
            panel.save("shortcuts", Edit::Set("shortcuts".into(), Value::Str((*id).into())));
        }
    });

    panel.reload();
    let scroll = gtk4::ScrolledWindow::new();
    scroll.connect_map(move |_| keys_state());
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&content));
    // O painel vive enquanto a página viver: o tratador do destroy é dono dele (os outros só têm o Weak)
    scroll.connect_destroy(move |_| {
        let _ = &panel;
    });
    scroll.upcast()
}
