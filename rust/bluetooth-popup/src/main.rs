//! Popup de Bluetooth para o clique no ícone da Waybar: o porte de `home/.local/bin/waybar-bluetooth-popup`,
//! que passa a vez a este binário (sway_rust). Liga e desliga o Bluetooth, deixa visível, lista os
//! aparelhos pareados, conecta, desconecta, procura e pareia novos, mostra detalhes, esquece um aparelho e
//! envia arquivos. Fala com o BlueZ pelo D-Bus.
//!
//! PIN E CONFIRMAÇÃO: este programa nunca lê nem digita códigos de pareamento. Quem responde é o agente do
//! blueman (blueman-agent), que abre o próprio diálogo.
//!
//! PAREAMENTO: o BlueZ cancela o pareamento se o programa que o pediu fechar, então o popup não fecha
//! enquanto pareia (a guarda do PopupWindow): o diálogo do agente tiraria o foco e cancelaria tudo. O
//! fechamento fica adiado até o fim.
//!
//! A busca só roda quando pedida: procurar aparelhos atrapalha o áudio Bluetooth. Ela para sozinha depois
//! de 30 s, ao parear ou quando o popup fecha (o BlueZ a encerra com o cliente).
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gio, glib, pango};
use swaydesk_bluez::{self as bluez, Device, State, ADAPTER, BLUEZ, DEVICE};
use swaydesk_common::i18n::tr;
use swaydesk_common::resident;
use swaydesk_ui::cmd::spawn_detached;
use swaydesk_ui::list::{empty_label, error_text, link_button, scroller, section, set_status, switch, CSS_DEV, CSS_LIST};
use swaydesk_ui::{label, on_terminate, run_popup, PopupWindow};

const PROGRAM: &str = "waybar-bluetooth-popup";
const PROPS: &str = "org.freedesktop.DBus.Properties";
const SCROLL_HEIGHT: i32 = 320;
const SCAN_SECONDS: u32 = 30;
const CONNECT_MS: i32 = 30_000;
/// Tempo para a pessoa responder ao diálogo do agente
const PAIR_MS: i32 = 90_000;
/// Tempo para ler o erro de um pareamento quando o fechamento foi adiado
const ERROR_SECONDS: u64 = 8;

// Material da Nerd Font
const GENERIC: &str = "\u{f00af}"; // bluetooth
const SEARCH: &str = "\u{f0349}"; // magnify
const STOP: &str = "\u{f04db}"; // stop
const CLOSE: &str = "\u{f0156}";
const MORE: &str = "\u{f01d9}"; // dots-vertical

fn glyph_of(icon: &str) -> &'static str {
    match icon {
        "audio-headphones" => "\u{f02cb}",
        "audio-headset" => "\u{f02ce}",
        "audio-card" | "audio-speakers" => "\u{f04c3}",
        "input-keyboard" => "\u{f030c}",
        "input-mouse" => "\u{f037d}",
        "input-gaming" => "\u{f0297}",
        "phone" => "\u{f011c}",
        "computer" => "\u{f0322}",
        "printer" => "\u{f042a}",
        "video-display" => "\u{f0502}",
        _ => GENERIC,
    }
}

fn kind_of(icon: &str) -> Option<String> {
    // O texto num let, e não dentro do tr(): o po/update (xgettext) tomaria o primeiro "..." do match
    let kind = match icon {
        "audio-headphones" => "Headphones",
        "audio-headset" => "Headset",
        "audio-card" => "Audio",
        "audio-speakers" => "Speaker",
        "input-keyboard" => "Keyboard",
        "input-mouse" => "Mouse",
        "input-gaming" => "Game controller",
        "phone" => "Phone",
        "computer" => "Computer",
        "printer" => "Printer",
        "video-display" => "Display",
        _ => return None,
    };
    Some(tr(kind))
}

const CSS: &str = r#"
.box { background: #38383d; border-radius: 10px; padding: 6px; }
button.dev:disabled { color: #dadada; }
button.action {
  background: #38383d; border: none; box-shadow: none; color: #dadada;
  padding: 5px 10px; min-height: 0; border-radius: 8px; font-size: 12px;
}
button.action:hover { background: #4c4c50; }
button.action.danger { background: #fc4138; color: #ffffff; }
button.link {
  background: transparent; border: none; box-shadow: none; color: #8a8a8f;
  padding: 0 4px; min-height: 0; font-size: 12px;
}
button.link:hover { color: #ffffff; }
"#;

type Action = Box<dyn Fn(&Rc<Popup>)>;
type View = (String, String, String, bool, Option<u8>, Option<String>);

struct Popup {
    popup: Rc<PopupWindow>,
    bus: Option<gio::DBusConnection>,
    state: RefCell<Option<State>>,
    sw_power: gtk4::Switch,
    sw_visible: gtk4::Switch,
    stack: gtk4::Stack,
    list: gtk4::Box,
    list_scroll: gtk4::ScrolledWindow,
    info: gtk4::Box,
    status: gtk4::Label,
    back: gtk4::Button,
    updating: Cell<bool>,
    pending: Cell<bool>,
    reading: Cell<bool>,
    again: Cell<bool>,
    loaded: Cell<bool>,
    sig: RefCell<Option<(Option<String>, bool, bool, Vec<View>, Vec<View>)>>,
    info_sig: RefCell<Option<(String, Vec<(String, String)>, Option<String>)>>,
    /// O aparelho da tela de detalhes
    detail: RefCell<Option<String>>,
    /// O aparelho com "Esquecer" esperando a confirmação
    forget_armed: RefCell<Option<String>>,
    /// Caminho → a operação em andamento
    busy: RefCell<HashMap<String, String>>,
    scanning: Cell<bool>,
    scan_timer: RefCell<Option<glib::SourceId>>,
    saw_discovering: Cell<bool>,
    pairing: Cell<u32>,
    close_pending: RefCell<Option<String>>,
    _subscriptions: RefCell<Vec<gio::SignalSubscriptionId>>,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Rc<Popup> {
        let popup = PopupWindow::new(PROGRAM, "waybar-bluetooth", &format!("{CSS_DEV}{CSS_LIST}{CSS}"), true, main_loop);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        card.add_css_class("card");
        card.set_size_request(360, -1);
        popup.window.set_child(Some(&card));

        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let sw_power = switch(&top, "Bluetooth");
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        top.append(&spacer);
        let sw_visible = switch(&top, &tr("Visible"));
        sw_visible.set_tooltip_text(Some(&tr("Lets other devices find this computer")));
        card.append(&top);

        let stack = gtk4::Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::None);
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let list_scroll = scroller(&list, SCROLL_HEIGHT);
        stack.add_named(&list_scroll, Some("list"));
        let info = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        stack.add_named(&scroller(&info, SCROLL_HEIGHT), Some("info"));
        card.append(&stack);

        // Linha de estado, sempre presente, para o aviso não mudar o tamanho
        let status = label(" ", &["status"], 0.0);
        status.set_ellipsize(pango::EllipsizeMode::End);
        card.append(&status);

        let this = Rc::new_cyclic(|me: &std::rc::Weak<Popup>| {
            let m = me.clone();
            let back = link_button(&tr("Back"), &tr("Back to the list of devices"), move || {
                if let Some(p) = m.upgrade() {
                    p.show_list();
                }
            });
            Popup {
                popup: popup.clone(),
                bus: bluez::system_bus().ok(),
                state: RefCell::new(None),
                sw_power,
                sw_visible,
                stack,
                list,
                list_scroll,
                info,
                status,
                back,
                updating: Cell::new(false),
                pending: Cell::new(false),
                reading: Cell::new(false),
                again: Cell::new(false),
                loaded: Cell::new(false),
                sig: RefCell::new(None),
                info_sig: RefCell::new(None),
                detail: RefCell::new(None),
                forget_armed: RefCell::new(None),
                busy: RefCell::new(HashMap::new()),
                scanning: Cell::new(false),
                scan_timer: RefCell::new(None),
                saw_discovering: Cell::new(false),
                pairing: Cell::new(0),
                close_pending: RefCell::new(None),
                _subscriptions: RefCell::new(Vec::new()),
            }
        });

        let foot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        foot.append(&this.back);
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        foot.append(&spacer);
        let me = Rc::downgrade(&this);
        foot.append(&link_button(&tr("More options"), &tr("Open the Bluetooth manager (blueman-manager)"), move || {
            if let Some(p) = me.upgrade() {
                p.open_app(&["blueman-manager"]);
            }
        }));
        card.append(&foot);

        let me = Rc::downgrade(&this);
        this.sw_power.connect_active_notify(move |sw| {
            let Some(p) = me.upgrade() else { return };
            let Some(adapter) = p.adapter_path() else { return };
            if !p.updating.get() {
                if !sw.is_active() {
                    p.cancel_scan();
                }
                p.bz_set(&adapter, ADAPTER, "Powered", sw.is_active());
            }
        });
        let me = Rc::downgrade(&this);
        this.sw_visible.connect_active_notify(move |sw| {
            let Some(p) = me.upgrade() else { return };
            let Some(adapter) = p.adapter_path() else { return };
            if !p.updating.get() {
                p.bz_set(&adapter, ADAPTER, "Discoverable", sw.is_active());
            }
        });

        // Esc na tela de detalhes volta à lista; na lista, o PopupWindow fecha. Na captura, antes dele
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let me = Rc::downgrade(&this);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(p) = me.upgrade() else { return glib::Propagation::Proceed };
            if key == gtk4::gdk::Key::Escape && p.detail.borrow().is_some() {
                p.show_list();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        this.popup.window.add_controller(keys);

        // Fechar enquanto pareia cancelaria o pareamento: o motivo fica guardado até o fim
        let me = Rc::downgrade(&this);
        this.popup.set_close_guard(move |reason| match me.upgrade() {
            Some(p) if p.pairing.get() > 0 => {
                p.close_pending.replace(Some(reason.to_string()));
                false
            }
            _ => true,
        });
        // O segundo clique no ícone também passa pela guarda
        let me = Rc::downgrade(&this);
        on_terminate(move || match me.upgrade() {
            Some(p) => {
                p.popup.close("clique no ícone");
                true
            }
            None => false, // sem o popup, o sinal encerra o processo
        });

        if let Some(bus) = &this.bus {
            let me = Rc::downgrade(&this);
            this._subscriptions.replace(bluez::watch(bus, move || {
                if let Some(p) = me.upgrade() {
                    p.schedule();
                }
            }));
        }
        this.refresh();
        this
    }

    fn adapter_path(&self) -> Option<String> {
        self.state.borrow().as_ref().and_then(|s| s.adapter.as_ref()).map(|a| a.path.clone())
    }

    fn set_status(&self, text: &str) {
        self.popup.rendered_at.set(Instant::now());
        set_status(&self.status, text);
    }

    /// Durante a busca o BlueZ manda rajadas de avisos (RSSI): uma leitura por rajada.
    fn schedule(self: &Rc<Self>) {
        if !self.pending.replace(true) {
            let me = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(250), move || {
                if let Some(p) = me.upgrade() {
                    p.pending.set(false);
                    p.refresh();
                }
            });
        }
    }

    fn refresh(self: &Rc<Self>) {
        if self.reading.replace(true) {
            self.again.set(true);
            return;
        }
        let bus = self.bus.clone();
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let state = gio::spawn_blocking(move || bus.and_then(|b| bluez::read(&b))).await.ok().flatten();
            let Some(p) = me.upgrade() else { return };
            p.reading.set(false);
            p.state.replace(state);
            p.sync_scan();
            p.sync_switches();
            p.fill_list();
            if p.detail.borrow().is_some() {
                p.fill_details();
            }
            if !p.loaded.replace(true) {
                p.popup.show();
                p.back.set_visible(false);
            }
            if p.again.replace(false) {
                p.schedule();
            }
        });
    }

    /// Acompanha o adaptador: a busca pode acabar por fora (Bluetooth desligado).
    fn sync_scan(&self) {
        if !self.scanning.get() {
            return;
        }
        let (discovering, powered) = self.state.borrow().as_ref().and_then(|s| s.adapter.as_ref()).map(|a| (a.discovering, a.powered)).unwrap_or((false, false));
        if discovering {
            self.saw_discovering.set(true);
        } else if self.saw_discovering.get() || !powered {
            self.clear_scan();
        }
    }

    fn clear_scan(&self) {
        self.scanning.set(false);
        self.saw_discovering.set(false);
        if let Some(timer) = self.scan_timer.take() {
            timer.remove();
        }
    }

    fn sync_switches(&self) {
        let state = self.state.borrow();
        let adapter = state.as_ref().and_then(|s| s.adapter.as_ref());
        self.updating.set(true);
        let powered = adapter.is_some_and(|a| a.powered);
        self.sw_power.set_active(powered);
        self.sw_power.set_sensitive(adapter.is_some());
        self.sw_visible.set_active(powered && adapter.is_some_and(|a| a.discoverable));
        self.sw_visible.set_sensitive(powered);
        self.updating.set(false);
    }

    fn view(&self, d: &Device) -> View {
        (d.path.clone(), d.alias.clone(), d.icon.clone(), d.connected, d.battery, self.busy.borrow().get(&d.path).cloned())
    }

    fn fill_list(self: &Rc<Self>) {
        let state = self.state.borrow().clone().unwrap_or_default();
        let adapter = state.adapter.as_ref();
        let powered = adapter.is_some_and(|a| a.powered);
        let (mine, others) = bluez::split_devices(&state.devices);
        let sig = (adapter.map(|a| a.path.clone()), powered, self.scanning.get(), mine.iter().map(|d| self.view(d)).collect(), others.iter().map(|d| self.view(d)).collect());
        if self.sig.borrow().as_ref() == Some(&sig) {
            return;
        }
        self.sig.replace(Some(sig));
        let keep = self.list_scroll.vadjustment().value();
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        if adapter.is_none() {
            self.list.append(&empty_label(&tr("No Bluetooth adapter")));
        } else if !powered {
            self.list.append(&empty_label(&tr("Bluetooth off")));
        } else {
            let mut rows: Vec<gtk4::Widget> = mine.iter().map(|d| self.device_row(d)).collect();
            if rows.is_empty() {
                rows.push(empty_label(&tr("No paired devices")).upcast());
            }
            self.list.append(&section(&tr("My devices"), &rows));
            let mut rows = vec![self.scan_row()];
            rows.extend(others.iter().map(|d| self.new_row(d)));
            self.list.append(&section(&tr("Other devices"), &rows));
        }
        self.popup.rendered_at.set(Instant::now());
        let adjustment = self.list_scroll.vadjustment();
        glib::idle_add_local_once(move || adjustment.set_value(keep));
    }

    fn make_row(self: &Rc<Self>, glyph: &str, title: &str, sub: &str, handler: Option<Action>, tip: Option<String>, css: Option<&str>, extras: Vec<(&str, String, Action)>) -> gtk4::Widget {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        let b = gtk4::Button::new();
        b.set_hexpand(true);
        b.add_css_class("dev");
        if let Some(css) = css {
            b.add_css_class(css);
        }
        let inner = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        inner.append(&label(glyph, &["glyph"], 0.5));
        let col = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
        let t = label(title, &["dev-name"], 0.0); // texto puro: o nome vem do aparelho
        t.set_ellipsize(pango::EllipsizeMode::End);
        col.append(&t);
        if !sub.is_empty() {
            let s = label(sub, &["dev-sub"], 0.0);
            s.set_ellipsize(pango::EllipsizeMode::End);
            col.append(&s);
        }
        inner.append(&col);
        b.set_child(Some(&inner));
        match handler {
            Some(handler) => {
                let me = Rc::downgrade(self);
                b.connect_clicked(move |_| {
                    if let Some(p) = me.upgrade() {
                        handler(&p);
                    }
                });
            }
            None => b.set_sensitive(false),
        }
        b.set_tooltip_text(tip.as_deref());
        row.append(&b);
        for (glyph, extra_tip, extra) in extras {
            let e = gtk4::Button::with_label(glyph);
            e.add_css_class("off");
            e.set_tooltip_text(Some(&extra_tip));
            let me = Rc::downgrade(self);
            e.connect_clicked(move |_| {
                if let Some(p) = me.upgrade() {
                    extra(&p);
                }
            });
            row.append(&e);
        }
        row.upcast()
    }

    fn device_row(self: &Rc<Self>, d: &Device) -> gtk4::Widget {
        let (path, name, glyph) = (d.path.clone(), d.title().to_string(), glyph_of(&d.icon));
        if let Some(busy) = self.busy.borrow().get(&path).cloned() {
            return self.make_row(glyph, &name, &busy, None, None, Some("busy"), vec![]);
        }
        let details_path = path.clone();
        if d.connected {
            let sub = match d.battery {
                Some(b) => tr("Connected · %s%%").replace("%s", &b.to_string()).replace("%%", "%"),
                None => tr("Connected"),
            };
            let off = path.clone();
            return self.make_row(glyph, &name, &sub, Some(Box::new(move |p: &Rc<Popup>| p.show_details(&details_path))), Some(tr("Show details")), Some("active"),
                vec![(CLOSE, tr("Disconnect"), Box::new(move |p: &Rc<Popup>| p.bz_disconnect(&off)) as Action)]);
        }
        self.make_row(glyph, &name, &tr("Paired"), Some(Box::new(move |p: &Rc<Popup>| p.bz_connect(&path, None))), Some(tr("Connect")), None,
            vec![(MORE, tr("Details"), Box::new(move |p: &Rc<Popup>| p.show_details(&details_path)) as Action)])
    }

    fn new_row(self: &Rc<Self>, d: &Device) -> gtk4::Widget {
        let (path, name, glyph) = (d.path.clone(), d.title().to_string(), glyph_of(&d.icon));
        if let Some(busy) = self.busy.borrow().get(&path).cloned() {
            return self.make_row(glyph, &name, &busy, None, None, Some("busy"), vec![]);
        }
        self.make_row(glyph, &name, &kind_of(&d.icon).unwrap_or_default(), Some(Box::new(move |p: &Rc<Popup>| p.pair_and_connect(&path))), Some(tr("Pair and connect")), None, vec![])
    }

    fn scan_row(self: &Rc<Self>) -> gtk4::Widget {
        if self.scanning.get() {
            return self.make_row(STOP, &tr("Searching..."), &tr("Click to stop"), Some(Box::new(|p: &Rc<Popup>| p.cancel_scan())), None, None, vec![]);
        }
        self.make_row(SEARCH, &tr("Search for devices"), "", Some(Box::new(|p: &Rc<Popup>| p.start_scan())),
            Some(tr("Searches for %d s; Bluetooth audio may stutter meanwhile").replace("%d", &SCAN_SECONDS.to_string())), None, vec![])
    }

    // --- detalhes ---
    fn show_details(self: &Rc<Self>, path: &str) {
        self.detail.replace(Some(path.to_string()));
        self.forget_armed.replace(None);
        self.info_sig.replace(None);
        self.fill_details();
        if self.detail.borrow().is_some() {
            self.stack.set_visible_child_name("info");
            self.back.set_visible(true);
        }
    }

    fn show_list(&self) {
        self.detail.replace(None);
        self.forget_armed.replace(None);
        self.stack.set_visible_child_name("list");
        self.back.set_visible(false);
    }

    fn fill_details(self: &Rc<Self>) {
        let Some(path) = self.detail.borrow().clone() else { return };
        let dev = self.state.borrow().as_ref().and_then(|s| s.devices.iter().find(|d| d.path == path).cloned());
        let Some(dev) = dev else {
            self.show_list(); // o aparelho foi esquecido ou sumiu
            return;
        };
        let (yes, no) = (tr("Yes"), tr("No"));
        let yn = |b: bool| if b { yes.clone() } else { no.clone() };
        let mut pairs = vec![
            (tr("Name"), dev.alias.clone()),
            (tr("Address"), dev.address.clone()),
            (tr("Type"), kind_of(&dev.icon).unwrap_or_else(|| tr("Other"))),
            (tr("Connected"), yn(dev.connected)),
            (tr("Paired"), yn(dev.paired)),
            (tr("Trusted"), yn(dev.trusted)),
        ];
        if let Some(b) = dev.battery {
            pairs.push((tr("Battery"), format!("{b}%")));
        }
        let sig = (path.clone(), pairs.clone(), self.forget_armed.borrow().clone());
        if self.info_sig.borrow().as_ref() == Some(&sig) {
            return;
        }
        self.info_sig.replace(Some(sig));
        while let Some(child) = self.info.first_child() {
            self.info.remove(&child);
        }
        let grid = gtk4::Grid::new();
        grid.set_column_spacing(12);
        grid.set_row_spacing(6);
        grid.add_css_class("box");
        for (i, (key, value)) in pairs.iter().enumerate() {
            let k = label(key, &["key"], 0.0);
            k.set_yalign(0.0);
            let v = label(value, &["value"], 0.0);
            v.set_selectable(true);
            v.set_focusable(false);
            v.set_wrap(true);
            v.set_wrap_mode(pango::WrapMode::Char);
            v.set_max_width_chars(30);
            v.set_hexpand(true);
            grid.attach(&k, 0, i as i32, 1, 1);
            grid.attach(&v, 1, i as i32, 1, 1);
        }
        self.info.append(&grid);

        let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let send = gtk4::Button::with_label(&tr("Send file"));
        send.add_css_class("action");
        let (me, address) = (Rc::downgrade(self), dev.address.clone());
        send.connect_clicked(move |_| {
            if let Some(p) = me.upgrade() {
                p.open_app(&["blueman-sendto", &format!("--device={address}")]);
            }
        });
        actions.append(&send);
        let armed = self.forget_armed.borrow().as_deref() == Some(path.as_str());
        let forget = gtk4::Button::with_label(&if armed { tr("Confirm: forget") } else { tr("Forget") });
        forget.add_css_class("action");
        if armed {
            forget.add_css_class("danger");
        }
        forget.set_tooltip_text(Some(&tr("Removes the pairing; to use it again, pair it again")));
        let me = Rc::downgrade(self);
        forget.connect_clicked(move |_| {
            if let Some(p) = me.upgrade() {
                p.on_forget(&path);
            }
        });
        actions.append(&forget);
        self.info.append(&actions);
        self.popup.rendered_at.set(Instant::now());
    }

    fn on_forget(self: &Rc<Self>, path: &str) {
        if self.forget_armed.borrow().as_deref() != Some(path) {
            self.forget_armed.replace(Some(path.to_string()));
            self.fill_details();
            return;
        }
        self.bz_remove(path);
    }

    // --- ações ---
    fn call(self: &Rc<Self>, path: &str, iface: &str, method: &str, args: Option<glib::Variant>, timeout: i32, done: impl FnOnce(&Rc<Popup>, Result<glib::Variant, glib::Error>) + 'static) {
        let Some(bus) = self.bus.clone() else { return };
        let me = Rc::downgrade(self);
        bus.call(Some(BLUEZ), path, iface, method, args.as_ref(), None, gio::DBusCallFlags::NONE, timeout, gio::Cancellable::NONE, move |result| {
            if let Some(p) = me.upgrade() {
                done(&p, result);
            }
        });
    }

    fn set_busy(self: &Rc<Self>, path: &str, text: Option<String>) {
        match text {
            Some(text) => self.busy.borrow_mut().insert(path.to_string(), text),
            None => self.busy.borrow_mut().remove(path),
        };
        self.schedule();
    }

    fn bz_connect(self: &Rc<Self>, path: &str, then: Option<Box<dyn FnOnce(&Rc<Popup>, bool)>>) {
        self.set_status("");
        self.set_busy(path, Some(tr("Connecting...")));
        let path = path.to_string();
        self.call(&path.clone(), DEVICE, "Connect", None, CONNECT_MS, move |p, result| {
            p.set_busy(&path, None);
            let ok = match result {
                Ok(_) => true,
                Err(err) => {
                    p.set_status(&tr("Could not connect: %s").replace("%s", &error_text(&err)));
                    false
                }
            };
            if let Some(then) = then {
                then(p, ok);
            }
        });
    }

    fn bz_disconnect(self: &Rc<Self>, path: &str) {
        self.set_status("");
        self.set_busy(path, Some(tr("Disconnecting...")));
        let path = path.to_string();
        self.call(&path.clone(), DEVICE, "Disconnect", None, CONNECT_MS, move |p, result| {
            p.set_busy(&path, None);
            if let Err(err) = result {
                p.set_status(&tr("Could not disconnect: %s").replace("%s", &error_text(&err)));
            }
        });
    }

    fn pair_and_connect(self: &Rc<Self>, path: &str) {
        if self.bus.is_none() {
            return; // sem a chamada, o contador do pareamento nunca voltaria a zero
        }
        self.set_status("");
        self.cancel_scan();
        self.pairing.set(self.pairing.get() + 1);
        // A camada do clique fora bloquearia o diálogo de confirmação do agente
        self.popup.drop_catchers();
        self.set_busy(path, Some(tr("Pairing: confirm in the dialog, if one appears")));
        let path = path.to_string();
        self.call(&path.clone(), DEVICE, "Pair", None, PAIR_MS, move |p, result| {
            if let Err(err) = result {
                let already = gio::DBusError::remote_error(&err).is_some_and(|name| name.contains("AlreadyExists"));
                if !already {
                    p.set_busy(&path, None);
                    p.set_status(&tr("Could not pair: %s").replace("%s", &error_text(&err)));
                    p.pair_finished(false);
                    return;
                }
            }
            // Confiável: reconecta sozinho, sem pedir autorização a cada vez
            p.bz_set(&path, DEVICE, "Trusted", true);
            p.bz_connect(&path, Some(Box::new(|p: &Rc<Popup>, ok| p.pair_finished(ok))));
        });
    }

    fn pair_finished(self: &Rc<Self>, ok: bool) {
        self.pairing.set(self.pairing.get().saturating_sub(1));
        if self.pairing.get() > 0 {
            return;
        }
        let Some(reason) = self.close_pending.take() else {
            self.popup.restore_catchers(); // ficou aberto: o clique fora volta a fechar
            return;
        };
        if ok {
            self.popup.close(&reason);
        } else {
            // Fechar já esconderia o erro, e a falha pareceria sucesso
            let me = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_secs(ERROR_SECONDS), move || {
                if let Some(p) = me.upgrade() {
                    p.popup.close(&reason);
                }
            });
        }
    }

    fn bz_remove(self: &Rc<Self>, path: &str) {
        self.set_status("");
        let Some(adapter) = self.adapter_path() else { return };
        self.show_list();
        let device = glib::variant::ObjectPath::try_from(path.to_string()).map(|o| o.to_variant());
        let Ok(device) = device else { return };
        self.call(&adapter, ADAPTER, "RemoveDevice", Some(glib::Variant::tuple_from_iter([device])), CONNECT_MS, |p, result| {
            if let Err(err) = result {
                p.set_status(&tr("Could not forget: %s").replace("%s", &error_text(&err)));
            }
            p.schedule();
        });
    }

    fn bz_set(self: &Rc<Self>, path: &str, iface: &str, prop: &str, value: bool) {
        let args = (iface, prop, value.to_variant()).to_variant();
        self.call(path, PROPS, "Set", Some(args), 10_000, |p, result| {
            if let Err(err) = result {
                p.set_status(&tr("Could not change: %s").replace("%s", &error_text(&err)));
            }
            p.schedule();
        });
    }

    fn bz_discovery(self: &Rc<Self>, start: bool) {
        let Some(adapter) = self.adapter_path() else { return };
        self.call(&adapter, ADAPTER, if start { "StartDiscovery" } else { "StopDiscovery" }, None, 10_000, move |p, result| {
            if let Err(err) = result {
                if start {
                    p.clear_scan();
                    p.set_status(&tr("Could not search: %s").replace("%s", &error_text(&err)));
                }
                p.schedule();
            }
        });
    }

    fn start_scan(self: &Rc<Self>) {
        self.set_status("");
        self.clear_scan();
        self.scanning.set(true);
        self.bz_discovery(true);
        let me = Rc::downgrade(self);
        let timer = glib::timeout_add_seconds_local_once(SCAN_SECONDS, move || {
            if let Some(p) = me.upgrade() {
                p.scan_timer.take(); // a fonte já terminou: o remove dela daria erro
                p.stop_scan();
            }
        });
        self.scan_timer.replace(Some(timer));
        self.schedule();
    }

    fn stop_scan(self: &Rc<Self>) {
        if self.scanning.get() {
            self.clear_scan();
            self.bz_discovery(false);
            self.schedule();
        }
    }

    /// Parada pedida antes do fim do tempo.
    fn cancel_scan(self: &Rc<Self>) {
        if let Some(timer) = self.scan_timer.take() {
            timer.remove();
        }
        self.stop_scan();
    }

    fn open_app(&self, argv: &[&str]) {
        match spawn_detached(argv[0], &argv[1..]) {
            Ok(()) => self.popup.close("app"),
            Err(err) => self.set_status(&tr("Could not open %(program)s: %(error)s").replace("%(program)s", argv[0]).replace("%(error)s", &err.to_string())),
        }
    }
}

/// Avisa o ícone (waybar-bluetooth), que esconde a dica com o popup aberto.
fn notify_icon() {
    resident::signal_pid(&resident::pidfile("waybar-bluetooth"), "waybar-bluetooth", libc::SIGUSR1);
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-bluetooth-popup em Python confere que o binário carrega
    }
    run_popup(PROGRAM, |main_loop| Some(Popup::new(main_loop)), notify_icon)
}
