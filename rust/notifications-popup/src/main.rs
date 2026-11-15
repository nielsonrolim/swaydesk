//! Popup da central de notificações, para o clique no ícone da Waybar e para o Super+C: o porte de
//! `home/.local/bin/waybar-notifications-popup`, que passa a vez a este binário (sway_rust).
//!
//! Quem guarda a lista é o sway-notifd: aqui só se lê e se pede por D-Bus. Clicar de novo no ícone, Esc,
//! clicar fora, focar outra janela ou trocar de workspace fecha.
//!
//! Tamanho constante: redimensionar a superfície faz o Sway tirar o foco, o que fecharia o popup. A
//! lista rola dentro de uma altura fixa.
use std::cell::{Cell, RefCell};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gio, glib, pango};
use serde_json::Value;
use swaydesk_common::i18n::{ptr, tr};
use swaydesk_notifd::widgets::{css, make_buttons, make_close, make_icon, make_text};
use swaydesk_notifd_logic::{group_by_app, relative_time};
use swaydesk_ui::list::{empty_label, error_text, link_button, scroller, set_status, Options, SCROLL_HEIGHT};
use swaydesk_ui::{label, run_popup, PopupWindow};

const PROGRAM: &str = "waybar-notifications-popup";
const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const CONTROL: &str = "local.SwayDesktop.Notifications1";
const CARD_WIDTH: i32 = 380;
const TEXT_CHARS: i32 = 32;
const CALL_MS: i32 = 3000;

const CSS: &str = r#"
.box { background: #38383d; border-radius: 10px; padding: 6px; }
.notif-list { background: transparent; }
.notif-list row {
  background: transparent; border: none; box-shadow: none; outline-style: none;
  border-radius: 8px; padding: 7px 8px; margin-bottom: 2px;
}
.notif-list row:last-child { margin-bottom: 0; }
.notif-list row.clickable:hover { background: #4c4c50; }
.notif-list button.off { padding: 2px 5px; color: #8a8a8f; opacity: 0; }
.notif-list row:hover button.off { opacity: 1; }
.notif-list button.off:hover { background: rgba(255, 255, 255, 0.12); color: #ffffff; }
.notif-list button.opt.action { background: #4c4c50; }
.notif-list button.opt.action:hover { background: #5a5a5f; }
.detail { color: #8a8a8f; font-size: 11px; }
"#;

fn now() -> f64 {
    glib::real_time() as f64 / 1e6
}

fn dnd_detail(dnd: &Value) -> String {
    let active = dnd["active"].as_bool().unwrap_or(false);
    let until = dnd["until"].as_f64();
    // Sem hora de fim, ligado é "até desligar"
    if dnd["choice"] == "on" || (active && until.is_none()) {
        return tr("On until you turn it off. Only critical ones show up");
    }
    if let (true, Some(until)) = (active, until) {
        let time = glib::DateTime::from_unix_local(until as i64).ok().and_then(|t| t.format("%H:%M").ok()).unwrap_or_default();
        return tr("On until %s. Only critical ones show up").replace("%s", &time);
    }
    " ".into() // sempre com texto: o tamanho não muda
}

struct Popup {
    popup: Rc<PopupWindow>,
    bus: Option<gio::DBusConnection>,
    body: gtk4::Box,
    scroll: gtk4::ScrolledWindow,
    status: gtk4::Label,
    clear_button: gtk4::Button,
    options: Options,
    detail: gtk4::Label,
    /// (rótulo do tempo, quando chegou), para atualizar a cada 30 s
    times: RefCell<Vec<(gtk4::Label, f64)>>,
    loaded: Cell<bool>,
    pending: Cell<bool>,
    /// O aviso do upper que devolve a rolagem depois de um apply; sai no apply seguinte, se não disparou
    scroll_handler: RefCell<Option<glib::SignalHandlerId>>,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Rc<Popup> {
        let popup = PopupWindow::new(PROGRAM, PROGRAM, &format!("{}{CSS}", css()), true, main_loop);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        card.add_css_class("card");
        card.set_size_request(CARD_WIDTH, -1);
        popup.window.set_child(Some(&card));

        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        let title = label(&tr("Notifications"), &["section"], 0.0);
        title.set_hexpand(true);
        head.append(&title);
        card.append(&head);

        let body = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let scroll = scroller(&body, SCROLL_HEIGHT);
        card.append(&scroll);

        let status = label(" ", &["status"], 0.0);
        status.set_ellipsize(pango::EllipsizeMode::End);
        card.append(&status);

        card.append(&label(&tr("Do not disturb"), &["section"], 0.0));
        let dnd_items = [
            ("off", ptr("do not disturb", "Off")),
            ("1h", "1 h".to_string()),
            ("2h", "2 h".to_string()),
            ("on", tr("Always")),
        ];
        let detail = label(" ", &["detail"], 0.0);

        let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE);
        let this = Rc::new_cyclic(|me: &std::rc::Weak<Popup>| {
            let weak = me.clone();
            let clear_button = link_button(&tr("Clear all"), &tr("Dismisses all notifications"), move || {
                if let Some(p) = weak.upgrade() {
                    p.call("ClearAll", None, |_, _| {});
                }
            });
            clear_button.set_can_focus(false);
            head.append(&clear_button);
            let weak = me.clone();
            let options = Options::new(&dnd_items, move |key| {
                if let Some(p) = weak.upgrade() {
                    p.call("SetDnd", Some((key,).to_variant()), |_, _| {});
                }
            });
            for button in options.buttons.values() {
                button.set_can_focus(false);
            }
            Popup {
                popup: popup.clone(),
                bus: bus.as_ref().ok().cloned(),
                body,
                scroll,
                status,
                clear_button,
                options,
                detail,
                times: RefCell::new(Vec::new()),
                loaded: Cell::new(false),
                pending: Cell::new(false),
                scroll_handler: RefCell::new(None),
            }
        });
        card.append(&this.options.widget);
        card.append(&this.detail);

        match &bus {
            Err(err) => this.fail(&error_text(err)),
            Ok(bus) => {
                let weak = Rc::downgrade(&this);
                #[allow(deprecated)] // o signal_subscribe com closure continua o caminho simples no gio 0.20
                bus.signal_subscribe(None, Some(CONTROL), Some("Changed"), Some(PATH), None, gio::DBusSignalFlags::NONE, move |_, _, _, _, _, _| {
                    if let Some(p) = weak.upgrade() {
                        p.schedule();
                    }
                });
                // A lista já está à vista: o daemon esconde os outros toasts e põe as críticas à esquerda
                this.call("CenterOpened", Some((CARD_WIDTH as u32,).to_variant()), |_, _| {});
                this.refresh();
                let weak = Rc::downgrade(&this);
                glib::timeout_add_seconds_local(30, move || match weak.upgrade() {
                    Some(p) => {
                        p.update_times();
                        glib::ControlFlow::Continue
                    }
                    None => glib::ControlFlow::Break,
                });
            }
        }
        this
    }

    /// Sem bloquear; o erro vai para a linha de status. `done` só roda se der certo.
    fn call(self: &Rc<Self>, method: &str, params: Option<glib::Variant>, done: impl FnOnce(&Rc<Popup>, glib::Variant) + 'static) {
        let Some(bus) = &self.bus else { return };
        let weak = Rc::downgrade(self);
        // Sem iniciar nada: o arquivo de ativação do pacote aponta para o swaync
        bus.call(
            Some(NAME),
            PATH,
            CONTROL,
            method,
            params.as_ref(),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            CALL_MS,
            gio::Cancellable::NONE,
            move |result| {
                let Some(p) = weak.upgrade() else { return };
                match result {
                    Ok(reply) => done(&p, reply),
                    Err(err) => p.fail(&error_text(&err)),
                }
            },
        );
    }

    /// Uma rajada de notificações vira uma leitura só.
    fn schedule(self: &Rc<Self>) {
        if !self.pending.replace(true) {
            let weak = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(100), move || {
                if let Some(p) = weak.upgrade() {
                    p.refresh();
                }
            });
        }
    }

    fn refresh(self: &Rc<Self>) {
        self.pending.set(false);
        self.call("List", None, |p, reply| {
            let snap = reply.child_value(0).str().and_then(|text| serde_json::from_str::<Value>(text).ok());
            match snap {
                Some(snap) => p.apply(&snap),
                None => p.fail("invalid reply"),
            }
        });
    }

    fn fail(&self, message: &str) {
        set_status(&self.status, &tr("Notification center unavailable: %s").replace("%s", message));
        self.reveal();
    }

    fn reveal(&self) {
        if !self.loaded.replace(true) {
            self.popup.show();
        }
    }

    fn invoke(self: &Rc<Self>, id: u32, key: &str) {
        self.call("Invoke", Some((id, key).to_variant()), |p, _| p.popup.close("ação"));
    }

    fn apply(self: &Rc<Self>, snap: &Value) {
        let dnd = &snap["dnd"];
        self.options.select(dnd["choice"].as_str().unwrap_or("off"));
        self.detail.set_text(&dnd_detail(dnd));
        set_status(&self.status, "");
        let position = self.scroll.vadjustment().value();
        // Um aviso de um apply que não mudou a altura não pode devolver a rolagem mais tarde
        if let Some(id) = self.scroll_handler.borrow_mut().take() {
            self.scroll.vadjustment().disconnect(id);
        }
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }
        self.times.borrow_mut().clear();
        let items: Vec<Value> = snap["items"].as_array().cloned().unwrap_or_default();
        for (app_name, group) in group_by_app(&items) {
            let section = self.make_group(&app_name, &group);
            self.body.append(&section);
        }
        if items.is_empty() {
            self.body.append(&empty_label(&tr("No notifications")));
        }
        self.clear_button.set_sensitive(!items.is_empty());
        // A posição volta agora e de novo quando o conteúdo novo medir (o upper muda no layout, depois
        // deste ponto): sem isso, a lista pula para o começo a cada Changed
        let adjustment = self.scroll.vadjustment();
        adjustment.set_value(position);
        let weak = Rc::downgrade(self);
        let id = adjustment.connect_upper_notify(move |adj| {
            adj.set_value(position);
            if let Some(id) = weak.upgrade().and_then(|p| p.scroll_handler.borrow_mut().take()) {
                adj.disconnect(id);
            }
        });
        *self.scroll_handler.borrow_mut() = Some(id);
        self.popup.rendered_at.set(Instant::now());
        self.reveal();
    }

    fn make_group(self: &Rc<Self>, app_name: &str, items: &[Value]) -> gtk4::Box {
        let section = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        let name = if app_name.is_empty() { tr("No name") } else { app_name.to_string() };
        let title = label(&name, &["section"], 0.0);
        title.set_ellipsize(pango::EllipsizeMode::End);
        title.set_max_width_chars(TEXT_CHARS);
        title.set_hexpand(true);
        head.append(&title);
        let who = if app_name.is_empty() { tr("no name") } else { app_name.to_string() };
        let weak = Rc::downgrade(self);
        let app = app_name.to_string();
        let link = link_button(&tr("Clear"), &tr("Dismisses the notifications from %s").replace("%s", &who), move || {
            if let Some(p) = weak.upgrade() {
                p.call("DismissApp", Some((app.as_str(),).to_variant()), |_, _| {});
            }
        });
        link.set_can_focus(false);
        head.append(&link);
        section.append(&head);

        let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        frame.add_css_class("box");
        let listbox = gtk4::ListBox::new();
        listbox.set_selection_mode(gtk4::SelectionMode::None);
        listbox.set_activate_on_single_click(true);
        listbox.set_can_focus(false);
        listbox.add_css_class("notif-list");
        for item in items {
            listbox.append(&self.make_row(item));
        }
        let weak = Rc::downgrade(self);
        listbox.connect_row_activated(move |_, row| {
            // Cada linha leva o id no nome: um filtro ou outra ordem não desencontram os dois
            let id = row.widget_name().parse::<u32>().ok();
            if let (Some(p), Some(id)) = (weak.upgrade(), id) {
                p.invoke(id, "default");
            }
        });
        frame.append(&listbox);
        section.append(&frame);
        section
    }

    fn make_row(self: &Rc<Self>, item: &Value) -> gtk4::ListBoxRow {
        let id = item["id"].as_u64().unwrap_or(0) as u32;
        let text = |key: &str| item[key].as_str().unwrap_or("").to_string();
        let has_default = item["has_default"].as_bool().unwrap_or(false);
        let row = gtk4::ListBoxRow::new();
        row.set_widget_name(&id.to_string());
        row.set_can_focus(false);
        row.set_activatable(has_default);
        if has_default {
            row.add_css_class("clickable");
        }
        let col = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        row.set_child(Some(&col));
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        top.append(&make_icon(&text("icon"), 32));
        let texts = make_text(&text("summary"), &text("body"), TEXT_CHARS);
        let created_at = item["created_at"].as_f64().unwrap_or(0.0);
        let when = label(&relative_time(created_at, now()), &["notif-time"], 0.0);
        texts.append(&when);
        self.times.borrow_mut().push((when, created_at));
        top.append(&texts);
        let weak = Rc::downgrade(self);
        top.append(&make_close(
            move || {
                if let Some(p) = weak.upgrade() {
                    p.call("Dismiss", Some((id,).to_variant()), |_, _| {});
                }
            },
            Some(&tr("Dismiss")),
        ));
        col.append(&top);
        let buttons: Vec<(String, String)> = item["buttons"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|pair| Some((pair[0].as_str()?.to_string(), pair[1].as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let weak = Rc::downgrade(self);
        if let Some(row_buttons) = make_buttons(&buttons, move |key| {
            if let Some(p) = weak.upgrade() {
                p.invoke(id, key);
            }
        }) {
            col.append(&row_buttons);
        }
        row
    }

    fn update_times(&self) {
        let now = now();
        for (when, created_at) in self.times.borrow().iter() {
            when.set_text(&relative_time(*created_at, now));
        }
    }
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-notifications-popup em Python confere que o binário carrega
    }
    run_popup(PROGRAM, |main_loop| Some(Popup::new(main_loop)), || {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dnd_detail_keeps_the_size() {
        assert_eq!(dnd_detail(&json!({"choice": "off", "active": false, "until": null})), " ");
        assert!(dnd_detail(&json!({"choice": "on", "active": true, "until": null})).contains("turn it off"));
        // Ativo sem hora de fim não vira "até 21:00" (a época no fuso local)
        assert!(dnd_detail(&json!({"choice": "1h", "active": true, "until": null})).contains("turn it off"));
    }
}
