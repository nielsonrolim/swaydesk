//! Os cartões da aba Usando o Swaydesk: o que cada app faz, o atalho de agora e o botão que o abre.
//!
//! O atalho vem do config em uso, achado pelo comando que o app roda: se o usuário trocar a tecla, o
//! cartão mostra a nova; se tirar, mostra "No shortcut". O botão roda um programa fixo, sem shell.
use gtk4::prelude::*;
use gtk4::{gdk, gio, pango};
use swaydesk_common::i18n::{ptr, tr};
use swaydesk_common::paths;
use swaydesk_help_logic::{find_shortcut, Entry};
use swaydesk_ui::cmd;

#[derive(Clone, Copy)]
pub enum Action {
    /// Sem botão: o app já está na tela (a dock) ou abre pelo atalho (o Alt+Tab).
    None,
    /// O app pelo .desktop: o GLib acha o Exec, e um app de outro pacote não depende do caminho.
    Desktop(&'static str),
    /// Outro componente: o lançador dos dotfiles ou o binário do pacote (paths::component).
    Component(&'static str, &'static str, &'static [&'static str]),
    /// Um script auxiliar (paths::helper): o de ~/.local/bin ou o da pasta dos scripts do pacote.
    Helper(&'static str, &'static [&'static str]),
}

pub struct Card {
    pub name: String,
    pub text: String,
    pub icon: &'static str,
    /// As palavras do comando do atalho ("sway-capture shot"); None: o cartão não mostra atalho.
    pub shortcut: Option<&'static str>,
    pub action: Action,
    /// O texto do botão; o padrão é "Open".
    pub button: Option<String>,
    /// Só aparece com o app instalado (o Telas, do swaydesk-displays).
    pub needs_desktop: Option<&'static str>,
}

pub fn cards() -> Vec<Card> {
    let card = |name: String, text: String, icon, shortcut, action| Card {
        name,
        text,
        icon,
        shortcut,
        action,
        button: None,
        needs_desktop: None,
    };
    vec![
        card(
            tr("Desktop Theme"),
            tr("The GTK theme, light or dark, and the accent color of the whole desktop: bar, dock, popups and apps."),
            "preferences-desktop-theme",
            None,
            Action::Desktop("org.swaydesktop.Theme.desktop"),
        ),
        card(
            tr("Screenshot"),
            tr("Capture a region, a window or the whole screen. In the panel, G opens the capture in Gradia to crop and annotate it."),
            "applets-screenshooter",
            Some("sway-capture shot"),
            Action::Component("sway-capture", "swaydesk-capture", &["shot"]),
        ),
        card(
            tr("Screen recording"),
            tr("Record a region, a window or the whole screen, with the system sound or the microphone. The red dot on the bar stops it."),
            "media-record",
            Some("sway-capture rec"),
            Action::Component("sway-capture", "swaydesk-capture", &["rec"]),
        ),
        card(
            tr("Clipboard"),
            tr("What you copied, text and images. Pick an item to copy it again."),
            "edit-paste",
            Some("waybar-clipboard-popup"),
            Action::Component("waybar-clipboard-popup", "swaydesk-clipboard-popup", &[]),
        ),
        card(
            tr("App grid"),
            tr("All the installed apps, with search and favorites. Super alone shows and hides it."),
            "view-app-grid",
            Some("sway-super"),
            Action::Component("sway-drawer", "swaydesk-drawer", &[]),
        ),
        card(
            tr("Dock"),
            tr("The favorite and open apps at the bottom of the screen. A click opens or focuses the app."),
            "user-desktop",
            None,
            Action::None,
        ),
        card(
            tr("Window switcher"),
            tr("The windows in the order you used them, from every workspace. Hold Alt and press Tab to move, then release to pick."),
            "preferences-system-windows",
            Some("sway-switcher next"),
            Action::None,
        ),
        card(
            tr("Notifications"),
            tr("The notifications you missed, kept until you clear them."),
            "preferences-system-notifications",
            Some("waybar-notifications-popup"),
            Action::Component("waybar-notifications-popup", "swaydesk-notifications-popup", &[]),
        ),
        Card {
            button: Some(tr("Lock now")),
            ..card(
                tr("Lock screen"),
                tr("Locks the screen with your password. It also locks by itself after some time idle and before suspending."),
                "system-lock-screen",
                Some("sway-lock"),
                Action::Helper("sway-lock", &[]),
            )
        },
        card(
            tr("Night light"),
            tr("Makes the screen warmer at night, always or on a schedule. It is in the brightness popup of the bar."),
            "weather-clear-night",
            None,
            Action::None,
        ),
        Card {
            needs_desktop: Some("org.swaydesktop.Displays.desktop"),
            ..card(
                tr("Displays"),
                tr("Resolution, scale and position of each monitor."),
                "preferences-desktop-display",
                None,
                Action::Desktop("org.swaydesktop.Displays.desktop"),
            )
        },
    ]
}

impl Card {
    pub fn available(&self) -> bool {
        self.needs_desktop.is_none_or(|id| gio::DesktopAppInfo::new(id).is_some())
    }

    pub fn widget(&self, entries: &[Entry], window: &gtk4::ApplicationWindow) -> gtk4::Box {
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        card.add_css_class("help-card");
        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        let icon = match self.action {
            Action::Desktop(id) => gio::DesktopAppInfo::new(id)
                .and_then(|info| info.icon())
                .map(|gicon| gtk4::Image::from_gicon(&gicon))
                .unwrap_or_else(|| gtk4::Image::from_icon_name(self.icon)),
            _ => gtk4::Image::from_icon_name(self.icon),
        };
        icon.set_pixel_size(32);
        head.append(&icon);
        let name = gtk4::Label::new(Some(&self.name));
        name.add_css_class("help-card-title");
        name.set_xalign(0.0);
        head.append(&name);
        card.append(&head);

        let text = gtk4::Label::new(Some(&self.text));
        text.set_wrap(true);
        text.set_wrap_mode(pango::WrapMode::WordChar);
        text.set_xalign(0.0);
        text.set_yalign(0.0);
        text.set_vexpand(true);
        text.set_max_width_chars(36);
        card.append(&text);

        let foot = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        if let Some(needle) = self.shortcut {
            match find_shortcut(entries, needle) {
                Some(chord) => foot.append(&super::chord_box(&chord)),
                None => {
                    let none = gtk4::Label::new(Some(&tr("No shortcut")));
                    none.add_css_class("help-dim");
                    foot.append(&none);
                }
            }
        }
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        foot.append(&spacer);
        if self.can_run() {
            // Com contexto: o "Open" sem ele é o da rede aberta (sem senha), um adjetivo
            let label = self.button.clone().unwrap_or_else(|| ptr("button", "Open"));
            let button = gtk4::Button::with_label(&label);
            let action = self.action;
            let window = window.downgrade();
            button.connect_clicked(move |_| {
                // Fecha antes de abrir: o app aberto (a grade, um popup) recebe o foco, e não a ajuda
                if let Some(window) = window.upgrade() {
                    window.close();
                }
                run(&action);
            });
            foot.append(&button);
        }
        card.append(&foot);
        card
    }

    fn can_run(&self) -> bool {
        match self.action {
            Action::None => false,
            Action::Desktop(id) => gio::DesktopAppInfo::new(id).is_some(),
            _ => true,
        }
    }
}

fn run(action: &Action) {
    let result = match action {
        Action::None => Ok(()),
        Action::Desktop(id) => match gio::DesktopAppInfo::new(id) {
            Some(info) => {
                let context = gdk::Display::default().map(|d| d.app_launch_context());
                if let Some(context) = &context {
                    cmd::clean_launch_context(context);
                }
                info.launch(&[], context.as_ref()).map_err(|e| e.message().to_string())
            }
            None => Err(format!("{id} is not installed")),
        },
        Action::Component(launcher, binary, args) => {
            cmd::spawn_detached(paths::component(launcher, binary), args).map_err(|e| e.to_string())
        }
        Action::Helper(name, args) => cmd::spawn_detached(cmd::bin(name), args).map_err(|e| e.to_string()),
    };
    if let Err(err) = result {
        eprintln!("swaydesk-help: could not open: {err}");
    }
}
