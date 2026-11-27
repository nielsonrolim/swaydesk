//! O painel Mouse e touchpad: a velocidade, a rolagem natural e a mão esquerda do mouse; no touchpad, também
//! tocar para clicar e desligar ao digitar. A seção do touchpad só aparece com um touchpad no GET_INPUTS. Os
//! valores mostrados são os que o Sway aplica agora (a seção libinput do dispositivo); sem dispositivo, os dos
//! arquivos. Grava só o que se muda aqui no 80-input.conf (input.rs) e aplica ao vivo, sem tocar no teclado (um
//! comando de teclado recompilaria o keymap e voltaria ao primeiro layout).
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::i18n::tr;
use swaydesk_settings_logic::input::{Device, Setting};
use swaydesk_settings_logic::mouse::live;

use crate::desktop::heading;
use crate::input::{effective, notes, snapshot, Applier, Snapshot};
use crate::region::line;

/// Os controles de um tipo de dispositivo.
struct Section {
    device: Device,
    title: gtk4::Box,
    list: gtk4::ListBox,
    speed: gtk4::Scale,
    natural: gtk4::Switch,
    left: gtk4::Switch,
    tap: Option<gtk4::Switch>,
    dwt: Option<gtk4::Switch>,
    /// A espera da barra de velocidade: grava quando ela para
    pending_speed: RefCell<Option<glib::SourceId>>,
}

struct Panel {
    applier: Rc<Applier>,
    snapshot: RefCell<Snapshot>,
    sections: Vec<Section>,
    hint: gtk4::Label,
    filling: Cell<bool>,
}

impl Panel {
    fn refresh(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let snapshot = gio::spawn_blocking(snapshot).await.unwrap_or_default();
            let Some(panel) = weak.upgrade() else { return };
            *panel.snapshot.borrow_mut() = snapshot;
            panel.fill();
        });
    }

    fn fill(&self) {
        self.filling.set(true);
        let snapshot = self.snapshot.borrow();
        let files = effective(snapshot.main_config.as_deref());
        for section in &self.sections {
            let devices = live(&snapshot.inputs, section.device);
            // O touchpad só com um touchpad; o mouse sempre (vale para o que for plugado)
            let shown = section.device == Device::Pointer || devices.is_some();
            section.title.set_visible(shown);
            section.list.set_visible(shown);
            let now = devices.unwrap_or_default();
            let value = |key: &str| {
                now.iter().chain(files.list(section.device)).find(|s| s.key() == key).cloned()
            };
            let flag = |key: &str| {
                use Setting::{DisableWhileTyping, LeftHanded, NaturalScroll, Tap};
                matches!(value(key), Some(NaturalScroll(true) | LeftHanded(true) | Tap(true) | DisableWhileTyping(true)))
            };
            let accel = match value("pointer_accel") {
                Some(Setting::Accel(accel)) => accel,
                _ => 0,
            };
            section.speed.set_value(f64::from(accel) / 100.0);
            section.natural.set_active(flag("natural_scroll"));
            section.left.set_active(flag("left_handed"));
            if let Some(tap) = &section.tap {
                tap.set_active(flag("tap"));
            }
            if let Some(dwt) = &section.dwt {
                dwt.set_active(flag("dwt"));
            }
        }
        let hint = notes(&snapshot, &[Device::Pointer, Device::Touchpad]);
        self.hint.set_visible(hint.is_some());
        self.hint.set_text(&hint.unwrap_or_default());
        self.filling.set(false);
    }

    fn set(self: &Rc<Self>, device: Device, setting: Setting) {
        if self.filling.get() {
            return;
        }
        let key = setting.key();
        self.applier.change(&[(device, key)], |conf| conf.set(device, setting));
    }
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

fn section(device: Device) -> Section {
    let touchpad = device == Device::Touchpad;
    let title = heading(&if touchpad { tr("Touchpad") } else { tr("Mouse") }, "");
    let speed = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, -1.0, 1.0, 0.05);
    speed.set_draw_value(false);
    speed.set_width_request(200);
    speed.add_mark(0.0, gtk4::PositionType::Bottom, None);
    let list = boxed_list();
    let natural = gtk4::Switch::new();
    let left = gtk4::Switch::new();
    append(&list, &line(&tr("Speed"), "", &speed));
    append(&list, &line(&tr("Natural scrolling"), &tr("The content moves with the fingers"), &natural));
    append(&list, &line(&tr("Left-handed"), &tr("The primary button is on the right"), &left));
    let (tap, dwt) = if touchpad {
        let (tap, dwt) = (gtk4::Switch::new(), gtk4::Switch::new());
        append(&list, &line(&tr("Tap to click"), "", &tap));
        append(&list, &line(&tr("Disable while typing"), "", &dwt));
        (Some(tap), Some(dwt))
    } else {
        (None, None)
    };
    Section { device, title, list, speed, natural, left, tap, dwt, pending_speed: RefCell::default() }
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

    let panel = Rc::new(Panel {
        applier,
        snapshot: RefCell::default(),
        sections: vec![section(Device::Pointer), section(Device::Touchpad)],
        hint: hint.clone(),
        filling: Cell::new(false),
    });
    for (i, section) in panel.sections.iter().enumerate() {
        content.append(&section.title);
        content.append(&section.list);
        let device = section.device;
        let toggles: Vec<(gtk4::Switch, fn(bool) -> Setting)> = [
            Some((section.natural.clone(), Setting::NaturalScroll as fn(bool) -> Setting)),
            Some((section.left.clone(), Setting::LeftHanded as fn(bool) -> Setting)),
            section.tap.clone().map(|s| (s, Setting::Tap as fn(bool) -> Setting)),
            section.dwt.clone().map(|s| (s, Setting::DisableWhileTyping as fn(bool) -> Setting)),
        ]
        .into_iter()
        .flatten()
        .collect();
        for (switch, make) in toggles {
            let weak = Rc::downgrade(&panel);
            switch.connect_active_notify(move |switch| {
                if let Some(panel) = weak.upgrade() {
                    panel.set(device, make(switch.is_active()));
                }
            });
        }
        let weak = Rc::downgrade(&panel);
        section.speed.connect_value_changed(move |scale| {
            let Some(panel) = weak.upgrade().filter(|p| !p.filling.get()) else { return };
            let section = &panel.sections[i];
            if let Some(source) = section.pending_speed.borrow_mut().take() {
                source.remove();
            }
            let accel = (scale.value() * 100.0).round() as i32;
            let weak = Rc::downgrade(&panel);
            let id = glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                let Some(panel) = weak.upgrade() else { return };
                panel.sections[i].pending_speed.borrow_mut().take();
                panel.set(device, Setting::Accel(accel));
            });
            *section.pending_speed.borrow_mut() = Some(id);
        });
    }
    content.append(&hint);
    // Embaixo: uma mensagem que aparece não empurra os controles
    content.append(&panel.applier.status);

    let scroll = gtk4::ScrolledWindow::new();
    {
        // Um mouse ou touchpad pode chegar com a janela aberta: lido de novo a cada vez que a página aparece
        let panel = panel.clone();
        scroll.connect_map(move |_| panel.refresh());
    }
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&content));
    scroll.upcast()
}
