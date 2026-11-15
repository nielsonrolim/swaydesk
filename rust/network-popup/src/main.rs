//! Popup de rede para o clique no ícone da Waybar: o porte de `home/.local/bin/waybar-network-popup`, que
//! passa a vez a este binário (sway_rust). Liga e desliga a rede e o Wi-Fi, lista as conexões cabeadas, as
//! VPNs e as redes Wi-Fi, conecta, desconecta e mostra os detalhes da conexão ativa (clique nela). Atualiza
//! sozinho pelos avisos do NetworkManager no D-Bus.
//!
//! Sem o NM no barramento, o mesmo popup fala com o iwd (Wi-Fi) e o systemd-networkd (cabo, só o estado):
//! ver `swaydesk_nm::standalone`. Ali não há VPN, editor de conexões nem o interruptor da rede, e eles
//! somem.
//!
//! SENHAS: com o NetworkManager, este programa nunca lê, recebe ou guarda senhas. Ele só pede a ativação ao
//! NM, que pergunta ao agente de segredos (o nm-applet, com --no-indicator). Não acrescente ao caminho do NM
//! campo de senha nem o `GetSecrets`.
//!
//! Só no backend iwd, que não tem agente próprio, o popup aberto é o agente (`net.connman.iwd.Agent`,
//! módulo `agent`): o iwd pede a senha de uma rede nova, o campo aparece no popup e o texto vai só na
//! resposta do D-Bus ao iwd. Nada de argv, log ou disco. Com o popup fechado não há agente: as redes
//! conhecidas o iwd conecta sozinho, e uma nova só pelo popup aberto. Outro agente já registrado (o iwctl, o
//! iwgtk) vale mais: o popup segue sem o campo.
//!
//! Tamanho constante: redimensionar a superfície faz o Sway tirar o foco, o que fecharia o popup. A lista
//! rola dentro de uma área de altura fixa.
use std::cell::{Cell, RefCell};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gio, glib, pango};
use swaydesk_common::i18n::tr;
use swaydesk_common::resident;
use swaydesk_nm::standalone as iwd;
use swaydesk_nm::{self as nm, active_state, device_state, Backend, Device, Network, Snapshot};

mod agent;
use agent::AgentState;
use swaydesk_ui::cmd::spawn_detached;
use swaydesk_ui::list::{empty_label, error_text, link_button, scroller, section, set_status, switch, CSS_DEV, CSS_LIST, SCROLL_HEIGHT};
use swaydesk_ui::{label, run_popup, PopupWindow};

const PROGRAM: &str = "waybar-network-popup";
const CLOSE: &str = "\u{f0156}";
const ADD: &str = "\u{f0415}";
const CALL_MS: i32 = 30_000;
const IFACE_ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";

/// Material da Nerd Font: wifi-strength-1..4 com cadeado
fn wifi_lock(bars: u8) -> &'static str {
    match bars {
        1 => "\u{f0921}",
        2 => "\u{f0924}",
        3 => "\u{f0927}",
        _ => "\u{f092a}",
    }
}

const CSS: &str = r#"
.box { background: #38383d; border-radius: 10px; padding: 6px; }
button.dev:disabled { color: #8a8a8f; }
button.link {
  background: transparent; border: none; box-shadow: none; color: #8a8a8f;
  padding: 0 4px; min-height: 0; font-size: 12px;
}
button.link:hover { color: #ffffff; }
"#;

/// O alvo da tela de detalhes.
#[derive(Clone, PartialEq)]
enum Detail {
    Wifi(String),
    Wired(String),
    /// Pelo uuid do perfil
    Vpn(String),
}

/// O que, na lista, muda o que se vê: só com isso diferente a lista é refeita (a rolagem não se perde).
type Signature = (Backend, Vec<(String, u32)>, Vec<(String, u32, Vec<(String, u8, bool, bool, String)>)>, Vec<(String, String, Option<u32>)>, Option<String>, bool);

struct Popup {
    popup: Rc<PopupWindow>,
    bus: Option<gio::DBusConnection>,
    snap: RefCell<Rc<Snapshot>>,
    sw_wifi: gtk4::Switch,
    sw_net: gtk4::Switch,
    /// O rótulo e o interruptor da rede, e os links do editor: só com o NM
    nm_only: RefCell<Vec<gtk4::Widget>>,
    /// O rótulo e o interruptor do Wi-Fi: com o NM, ou com uma placa no iwd
    wifi_toggle: Vec<gtk4::Widget>,
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
    sig: RefCell<Option<Signature>>,
    detail: RefCell<Option<Detail>>,
    /// A ativação pedida por este popup, ainda sem resultado (o caminho do Active)
    activation: RefCell<Option<String>>,
    /// Quando a ativação foi guardada: só uma leitura começada depois pode dizer que ela sumiu
    activation_at: Cell<Option<Instant>>,
    /// O nome do que se pediu para ativar, para a mensagem de falha se o Active já tiver sumido
    activation_name: RefCell<String>,
    /// O último (estado, motivo) de cada Active, dos avisos: uma falha imediata chega antes da resposta
    /// do pedido, e o finish_activation confere aqui
    active_states: RefCell<std::collections::HashMap<String, (u32, u32)>>,
    _subscriptions: RefCell<Vec<gio::SignalSubscriptionId>>,
    /// O agente de senhas do iwd (só no backend iwd)
    agent: agent::Agent,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Rc<Popup> {
        let popup = PopupWindow::new(PROGRAM, "waybar-network", &format!("{CSS_DEV}{CSS_LIST}{CSS}"), true, main_loop);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        card.add_css_class("card");
        card.set_size_request(360, -1);
        popup.window.set_child(Some(&card));

        // Interruptores
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let sw_wifi = switch(&top, "Wi-Fi");
        let wifi_toggle: Vec<gtk4::Widget> = [sw_wifi.prev_sibling(), Some(sw_wifi.clone().upcast())].into_iter().flatten().collect();
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        top.append(&spacer);
        let sw_net = switch(&top, &tr("Network"));
        let mut nm_only: Vec<gtk4::Widget> = [sw_net.prev_sibling(), Some(sw_net.clone().upcast())].into_iter().flatten().collect();
        card.append(&top);

        // Lista e detalhes trocam de lugar sem mudar o tamanho da janela
        let stack = gtk4::Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::None);
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let list_scroll = scroller(&list, SCROLL_HEIGHT);
        stack.add_named(&list_scroll, Some("list"));
        let info = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        stack.add_named(&scroller(&info, SCROLL_HEIGHT), Some("info"));
        let agent = agent::Agent::new();
        stack.add_named(&agent.page, Some("password"));
        card.append(&stack);

        // Linha de estado, sempre presente, para o aviso não mudar o tamanho
        let status = label(" ", &["status"], 0.0);
        status.set_ellipsize(pango::EllipsizeMode::End);
        card.append(&status);

        let this = Rc::new_cyclic(|me: &std::rc::Weak<Popup>| {
            let m = me.clone();
            let back = link_button(&tr("Back"), &tr("Back to the list of networks"), move || {
                if let Some(p) = m.upgrade() {
                    p.show_list();
                }
            });
            Popup {
                popup: popup.clone(),
                bus: nm::system_bus().ok(),
                snap: RefCell::new(Rc::new(Snapshot::default())),
                sw_wifi,
                sw_net,
                nm_only: RefCell::new(Vec::new()),
                wifi_toggle,
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
                detail: RefCell::new(None),
                activation: RefCell::new(None),
                activation_at: Cell::new(None),
                activation_name: RefCell::new(String::new()),
                active_states: RefCell::new(std::collections::HashMap::new()),
                _subscriptions: RefCell::new(Vec::new()),
                agent,
            }
        });

        let foot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        foot.append(&this.back);
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        foot.append(&spacer);
        let me = Rc::downgrade(&this);
        let hidden = link_button(&tr("Hidden network"), &tr("Create a connection to a hidden or enterprise network"), move || {
            if let Some(p) = me.upgrade() {
                p.open_editor(&["--create", "--type=802-11-wireless"]);
            }
        });
        foot.append(&hidden);
        let me = Rc::downgrade(&this);
        let edit = link_button(&tr("Edit connections"), &tr("Open the connection editor"), move || {
            if let Some(p) = me.upgrade() {
                p.open_editor(&[]);
            }
        });
        foot.append(&edit);
        card.append(&foot);
        nm_only.extend([hidden.upcast(), edit.upcast()]);
        this.nm_only.replace(nm_only);

        for (sw, command) in [(&this.sw_wifi, ["radio", "wifi"].as_slice()), (&this.sw_net, ["networking"].as_slice())] {
            let me = Rc::downgrade(&this);
            let wifi = std::ptr::eq(sw, &this.sw_wifi);
            let command: Vec<String> = command.iter().map(|s| s.to_string()).collect();
            sw.connect_active_notify(move |sw| {
                let Some(p) = me.upgrade() else { return };
                if p.updating.get() {
                    return;
                }
                if p.snap.borrow().backend == Backend::Standalone {
                    if wifi {
                        p.iwd_power(sw.is_active());
                    }
                    return;
                }
                let mut args = command.clone();
                args.push(if sw.is_active() { "on" } else { "off" }.into());
                p.spawn("nmcli", &args);
            });
        }

        // Esc na tela de detalhes volta à lista; na lista, o PopupWindow fecha. Na captura, antes dele
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let me = Rc::downgrade(&this);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(p) = me.upgrade() else { return glib::Propagation::Proceed };
            if key == gtk4::gdk::Key::Escape && p.agent.asking() {
                p.agent.cancel();
                p.show_list();
                return glib::Propagation::Stop;
            }
            if key == gtk4::gdk::Key::Escape && p.detail.borrow().is_some() {
                p.show_list();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        this.popup.window.add_controller(keys);

        // Fechar com a senha pedida responde "cancelado" ao iwd, que não fica esperando
        let me = Rc::downgrade(&this);
        this.popup.set_close_guard(move |_| {
            if let Some(p) = me.upgrade() {
                p.agent.cancel();
            }
            true
        });
        let me = Rc::downgrade(&this);
        this.agent.connect(
            this.bus.clone(),
            move |event| {
                if let Some(p) = me.upgrade() {
                    p.on_agent(event);
                }
            },
        );

        this.watch();
        this.refresh(true);
        this
    }

    fn spawn(&self, program: &str, args: &[String]) {
        if let Err(err) = spawn_detached(program, args) {
            self.set_status(&tr("Could not open %(program)s: %(error)s").replace("%(program)s", program).replace("%(error)s", &err.to_string()));
        }
    }

    fn set_status(&self, text: &str) {
        self.popup.rendered_at.set(Instant::now());
        set_status(&self.status, text);
    }

    /// Os avisos do NM: tudo relê (agrupado); o StateChanged da ativação pedida aqui diz se ela falhou.
    fn watch(self: &Rc<Self>) {
        let Some(bus) = &self.bus else { return };
        let me = Rc::downgrade(self);
        let mut subs = nm::watch(bus, move || {
            if let Some(p) = me.upgrade() {
                p.schedule();
            }
        });
        let me = Rc::downgrade(self);
        #[allow(deprecated)]
        subs.push(bus.signal_subscribe(Some(nm::NM), Some(IFACE_ACTIVE), Some("StateChanged"), None, None, gio::DBusSignalFlags::NONE, move |_, _, path, _, _, params| {
            if let (Some(p), Some((state, reason))) = (me.upgrade(), params.get::<(u32, u32)>()) {
                p.on_active_state(path, state, reason);
            }
        }));
        // O iwd reiniciado (ou fora do ar) esquece o agente: o próximo refresh registra de novo
        let me = Rc::downgrade(self);
        #[allow(deprecated)]
        subs.push(bus.signal_subscribe(Some("org.freedesktop.DBus"), Some("org.freedesktop.DBus"), Some("NameOwnerChanged"), Some("/org/freedesktop/DBus"), Some(iwd::IWD), gio::DBusSignalFlags::NONE, move |_, _, _, _, _, _| {
            if let Some(p) = me.upgrade() {
                p.agent.reset();
            }
        }));
        self._subscriptions.replace(subs);
    }

    /// Agrupa a rajada de avisos num refresh só.
    fn schedule(self: &Rc<Self>) {
        if !self.pending.replace(true) {
            let me = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(150), move || {
                if let Some(p) = me.upgrade() {
                    p.pending.set(false);
                    p.refresh(false);
                }
            });
        }
    }

    fn refresh(self: &Rc<Self>, scan: bool) {
        if self.reading.replace(true) {
            self.again.set(true);
            return;
        }
        let bus = self.bus.clone();
        let me = Rc::downgrade(self);
        let started = Instant::now();
        glib::MainContext::default().spawn_local(async move {
            let snap = gio::spawn_blocking(move || bus.map(|b| nm::read(&b, true)).unwrap_or_default()).await.unwrap_or_default();
            let Some(p) = me.upgrade() else { return };
            p.reading.set(false);
            p.apply(Rc::new(snap), started);
            if scan {
                p.request_scans();
            }
            if p.again.replace(false) {
                p.schedule();
            }
        });
    }

    /// O NM recusa varreduras seguidas; a lista atual continua valendo. O iwd também (InProgress, Busy).
    fn request_scans(&self) {
        let Some(bus) = &self.bus else { return };
        let snap = self.snap.borrow();
        if snap.backend == Backend::Standalone {
            for dev in nm::wifi_devices(&snap).into_iter().filter(|d| d.state != device_state::UNAVAILABLE) {
                bus.call(Some(iwd::IWD), &dev.path, iwd::IWD_STATION, "Scan", None, None, gio::DBusCallFlags::NO_AUTO_START, CALL_MS, gio::Cancellable::NONE, |_| {});
            }
            return;
        }
        for dev in nm::wifi_devices(&snap) {
            let args = glib::Variant::tuple_from_iter([glib::VariantDict::new(None).end()]);
            bus.call(Some(nm::NM), &dev.path, "org.freedesktop.NetworkManager.Device.Wireless", "RequestScan", Some(&args), None, gio::DBusCallFlags::NONE, CALL_MS, gio::Cancellable::NONE, |_| {});
        }
    }

    fn apply(self: &Rc<Self>, snap: Rc<Snapshot>, started: Instant) {
        self.snap.replace(snap.clone());
        // Uma ativação que some sem passar por "ativada" falhou, mesmo sem aviso de estado (uma VPN sem o
        // plugin cai antes de o sinal chegar). Só vale uma leitura começada depois de o pedido voltar
        let activated = self.activation.borrow().as_deref().is_some_and(|path| snap.active(Some(path)).is_some_and(|a| a.state == active_state::ACTIVATED));
        if activated {
            self.activation.replace(None); // conectou, mesmo que o aviso de estado tenha se perdido
        }
        let gone = self.activation.borrow().as_deref().is_some_and(|path| snap.active(Some(path)).is_none());
        if gone && self.activation_at.get().is_some_and(|at| started > at) {
            self.activation.replace(None);
            self.set_status(&tr("Could not connect to %s").replace("%s", &self.activation_name.borrow()));
        }
        self.sync_switches(&snap);
        self.agent.want(snap.backend == Backend::Standalone && !nm::wifi_devices(&snap).is_empty());
        self.fill_list(&snap);
        if self.detail.borrow().is_some() {
            self.fill_details();
        }
        if !self.loaded.replace(true) {
            self.popup.show();
            self.back.set_visible(false);
        }
    }

    fn sync_switches(&self, snap: &Snapshot) {
        let standalone = snap.backend == Backend::Standalone;
        for w in self.nm_only.borrow().iter() {
            w.set_visible(!standalone);
        }
        for w in &self.wifi_toggle {
            w.set_visible(!standalone || !nm::wifi_devices(snap).is_empty());
        }
        self.updating.set(true);
        self.sw_net.set_active(snap.running && snap.networking);
        self.sw_net.set_sensitive(snap.running);
        self.sw_wifi.set_active(snap.running && snap.wireless);
        self.sw_wifi.set_sensitive(snap.running && snap.wireless_hardware && snap.networking);
        self.updating.set(false);
    }

    fn signature(snap: &Snapshot) -> Signature {
        let wired = nm::wired_devices(snap).iter().map(|d| (d.iface.clone(), d.state)).collect();
        let wifi = nm::wifi_devices(snap)
            .iter()
            .map(|d| {
                let rows = nm::group_networks(snap, d)
                    .iter()
                    .map(|r| (r.ssid.clone(), r.bars, !r.saved.is_empty(), r.active, nm::security_of(r.ap).0))
                    .collect();
                (d.iface.clone(), d.state, rows)
            })
            .collect();
        let vpns = nm::vpn_rows(snap).iter().map(|(c, a)| (c.uuid.clone(), c.id.clone(), a.map(|a| a.state))).collect();
        (snap.backend, wired, wifi, vpns, nm::wifi_blocked(snap), snap.running)
    }

    fn fill_list(self: &Rc<Self>, snap: &Rc<Snapshot>) {
        let sig = Self::signature(snap);
        if self.sig.borrow().as_ref() == Some(&sig) {
            return;
        }
        self.sig.replace(Some(sig));
        let keep = self.list_scroll.vadjustment().value();
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        if let Some(text) = nm::not_running(snap) {
            self.list.append(&empty_label(&text));
        } else {
            let standalone = snap.backend == Backend::Standalone;
            if snap.iwd_denied {
                self.list.append(&empty_label(&tr("No permission to use iwd")));
            }
            let wired = nm::wired_devices(snap);
            if !wired.is_empty() {
                let rows: Vec<gtk4::Widget> = wired.iter().map(|d| self.wired_row(snap, d)).collect();
                self.list.append(&section(&tr("Wired"), &rows));
            }
            // Acima do Wi-Fi: a lista de redes é longa e esconderia a VPN. Sem o NM não há VPN
            if !standalone {
                let mut rows: Vec<gtk4::Widget> = nm::vpn_rows(snap).iter().map(|(c, a)| self.vpn_row(c, *a)).collect();
                rows.push(self.add_vpn_row());
                self.list.append(&section("VPN", &rows));
            }
            if standalone && wired.is_empty() && nm::wifi_devices(snap).is_empty() && !snap.iwd_denied {
                self.list.append(&empty_label(&tr("Not connected")));
            }
            let wifi = nm::wifi_devices(snap);
            for dev in &wifi {
                let title = if wifi.len() == 1 { "Wi-Fi".to_string() } else { format!("Wi-Fi ({})", dev.iface) };
                let nets = nm::group_networks(snap, dev);
                let shown: Vec<gtk4::Widget> = match nm::wifi_blocked(snap) {
                    Some(blocked) => vec![empty_label(&blocked).upcast()],
                    None if nets.is_empty() => vec![empty_label(&tr("Searching for networks...")).upcast()],
                    None => nets.iter().map(|r| self.wifi_row(dev, r)).collect(),
                };
                self.list.append(&section(&title, &shown));
            }
        }
        self.popup.rendered_at.set(Instant::now());
        let adjustment = self.list_scroll.vadjustment();
        glib::idle_add_local_once(move || adjustment.set_value(keep));
    }

    /// Uma linha: o botão (glifo, título e subtítulo) e, se houver, o botão de desligar.
    fn make_row(
        self: &Rc<Self>,
        glyph: &str,
        title: &str,
        sub: &str,
        handler: Option<Box<dyn Fn(&Rc<Popup>)>>,
        tip: Option<String>,
        css: Option<&str>,
        off: Option<(Box<dyn Fn(&Rc<Popup>)>, String)>,
    ) -> gtk4::Widget {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let b = gtk4::Button::new();
        b.set_hexpand(true);
        b.add_css_class("dev");
        if let Some(css) = css {
            b.add_css_class(css);
        }
        let inner = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        inner.append(&label(glyph, &["glyph"], 0.5));
        let col = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
        let t = label(title, &["dev-name"], 0.0); // texto puro: o SSID não é interpretado como marcação
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
        if let Some((on_off, off_tip)) = off {
            let button = gtk4::Button::with_label(CLOSE);
            button.add_css_class("off");
            button.set_tooltip_text(Some(&off_tip));
            let me = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(p) = me.upgrade() {
                    on_off(&p);
                }
            });
            row.append(&button);
        }
        row.upcast()
    }

    fn wifi_row(self: &Rc<Self>, dev: &Device, r: &Network) -> gtk4::Widget {
        let (sec, _) = nm::security_of(r.ap);
        let glyph = if nm::locked(r.ap) { wifi_lock(r.bars) } else { nm::wifi_icon(r.bars, false) };
        let path = dev.path.clone();
        let disconnect = |p: String| -> Box<dyn Fn(&Rc<Popup>)> { Box::new(move |me: &Rc<Popup>| me.disconnect(&p)) };
        if r.active && dev.state == device_state::ACTIVATED {
            let detail = path.clone();
            return self.make_row(glyph, &r.ssid, &tr("Connected · %s").replace("%s", &sec),
                Some(Box::new(move |p: &Rc<Popup>| p.show_details(Detail::Wifi(detail.clone())))), Some(tr("Show details")), Some("active"),
                Some((disconnect(path), tr("Disconnect"))));
        }
        if r.active && dev.state == device_state::NEED_AUTH {
            return self.make_row(glyph, &r.ssid, &tr("Waiting for password"), None, None, Some("busy"), Some((disconnect(path), tr("Cancel"))));
        }
        if r.active && device_state::CONNECTING.contains(&dev.state) {
            return self.make_row(glyph, &r.ssid, &tr("Connecting..."), None, None, Some("busy"), Some((disconnect(path), tr("Cancel"))));
        }
        let sub = if r.saved.is_empty() { sec } else { tr("%s · saved").replace("%s", &sec) };
        let ssid = r.ssid.clone();
        self.make_row(glyph, &r.ssid, &sub, Some(Box::new(move |p: &Rc<Popup>| p.connect_wifi(&path, &ssid))), Some(tr("Connect")), None, None)
    }

    fn wired_row(self: &Rc<Self>, snap: &Snapshot, dev: &Device) -> gtk4::Widget {
        if snap.backend == Backend::Standalone {
            return self.networkd_row(snap, dev);
        }
        let (name, path) = (dev.iface.clone(), dev.path.clone());
        let p2 = path.clone();
        let disconnect: Box<dyn Fn(&Rc<Popup>)> = Box::new(move |me: &Rc<Popup>| me.nm_disconnect(&p2));
        if dev.state == device_state::ACTIVATED {
            let id = snap.active(dev.active_connection.as_deref()).map(|a| a.id.clone()).unwrap_or_else(|| name.clone());
            return self.make_row(nm::ETHERNET, &id, &tr("Connected · %s").replace("%s", &name),
                Some(Box::new(move |p: &Rc<Popup>| p.show_details(Detail::Wired(path.clone())))), Some(tr("Show details")), Some("active"),
                Some((disconnect, tr("Disconnect"))));
        }
        if device_state::CONNECTING.contains(&dev.state) || dev.state == device_state::NEED_AUTH {
            return self.make_row(nm::ETHERNET, &name, &tr("Connecting..."), None, None, Some("busy"), Some((disconnect, tr("Cancel"))));
        }
        if dev.state == device_state::UNAVAILABLE {
            return self.make_row(nm::ETHERNET, &name, &tr("Cable unplugged"), None, None, None, None);
        }
        let label_name = name.clone();
        self.make_row(nm::ETHERNET, &name, &tr("Disconnected"), Some(Box::new(move |p: &Rc<Popup>| {
            p.activation_name.replace(label_name.clone());
            p.nm_activate(None, Some(&path), None)
        })), Some(tr("Connect")), None, None)
    }

    /// O cabo pelo networkd: só o estado (quem liga e desliga é o .network dele) e os detalhes.
    fn networkd_row(self: &Rc<Self>, snap: &Snapshot, dev: &Device) -> gtk4::Widget {
        let name = dev.iface.clone();
        if dev.state == device_state::ACTIVATED {
            let id = snap.active(dev.active_connection.as_deref()).map(|a| a.id.clone()).unwrap_or_else(|| name.clone());
            let path = dev.path.clone();
            return self.make_row(nm::ETHERNET, &id, &tr("Connected · %s").replace("%s", &name),
                Some(Box::new(move |p: &Rc<Popup>| p.show_details(Detail::Wired(path.clone())))), Some(tr("Show details")), Some("active"), None);
        }
        if device_state::CONNECTING.contains(&dev.state) {
            return self.make_row(nm::ETHERNET, &name, &tr("Connecting..."), None, None, Some("busy"), None);
        }
        if dev.state == device_state::UNAVAILABLE {
            return self.make_row(nm::ETHERNET, &name, &tr("Cable unplugged"), None, None, None, None);
        }
        self.make_row(nm::ETHERNET, &name, &tr("Disconnected"), None, None, None, None)
    }

    fn add_vpn_row(self: &Rc<Self>) -> gtk4::Widget {
        self.make_row(ADD, &tr("Add VPN"), "", Some(Box::new(|p: &Rc<Popup>| p.open_editor(&["--create", "--type=vpn"]))),
            Some(tr("Create or import a VPN in the connection editor")), None, None)
    }

    fn vpn_row(self: &Rc<Self>, conn: &nm::Connection, active: Option<&nm::Active>) -> gtk4::Widget {
        let kind = nm::vpn_type(conn);
        let Some(active) = active else {
            let (path, id) = (conn.path.clone(), conn.id.clone());
            return self.make_row(nm::VPN, &conn.id, &tr("%s · disconnected").replace("%s", &kind),
                Some(Box::new(move |p: &Rc<Popup>| {
                    p.activation_name.replace(id.clone());
                    p.nm_activate(Some(&path), None, None)
                })), Some(tr("Connect")), None, None);
        };
        let ap = active.path.clone();
        let off: Box<dyn Fn(&Rc<Popup>)> = Box::new(move |p: &Rc<Popup>| p.nm_deactivate(&ap));
        if active.state == active_state::ACTIVATED {
            let uuid = conn.uuid.clone();
            return self.make_row(nm::VPN, &conn.id, &tr("%s · connected").replace("%s", &kind),
                Some(Box::new(move |p: &Rc<Popup>| p.show_details(Detail::Vpn(uuid.clone())))), Some(tr("Show details")), Some("active"),
                Some((off, tr("Disconnect"))));
        }
        self.make_row(nm::VPN, &conn.id, &tr("Connecting..."), None, None, Some("busy"), Some((off, tr("Cancel"))))
    }

    // --- detalhes ---
    fn show_details(self: &Rc<Self>, detail: Detail) {
        self.detail.replace(Some(detail));
        self.fill_details();
        if self.detail.borrow().is_some() {
            self.stack.set_visible_child_name("info");
            self.back.set_visible(true);
        }
    }

    fn show_list(&self) {
        self.detail.replace(None);
        self.stack.set_visible_child_name("list");
        self.back.set_visible(false);
    }

    fn fill_details(self: &Rc<Self>) {
        let Some(detail) = self.detail.borrow().clone() else { return };
        let snap = self.snap.borrow().clone();
        let (active, mut pairs): (Option<&nm::Active>, Vec<(String, String)>) = match &detail {
            Detail::Vpn(uuid) => {
                let conn = snap.connections.iter().find(|c| &c.uuid == uuid);
                let active = snap.actives.iter().find(|a| &a.uuid == uuid);
                let pairs = conn.map(|c| vec![(tr("Connection"), c.id.clone()), (tr("Type"), nm::vpn_type(c))]).unwrap_or_default();
                (active.filter(|_| conn.is_some()), pairs)
            }
            Detail::Wifi(path) | Detail::Wired(path) => match snap.device(path) {
                None => (None, Vec::new()),
                Some(dev) => {
                    let active = snap.active(dev.active_connection.as_deref());
                    let mut pairs = vec![
                        (tr("Connection"), active.map(|a| a.id.clone()).unwrap_or_default()),
                        (tr("Interface"), dev.iface.clone()),
                        ("MAC".to_string(), dev.hw_address.clone()),
                    ];
                    if matches!(detail, Detail::Wifi(_)) {
                        if let Some(ap) = snap.access_point(dev.active_access_point.as_deref()) {
                            pairs.extend([
                                (tr("Network"), ap.ssid.clone()),
                                (tr("Security"), nm::security_of(ap).0),
                                (tr("Signal"), format!("{}%", ap.strength)),
                                (tr("Frequency"), format!("{} MHz ({})", ap.frequency, nm::freq_band(ap.frequency))),
                                (tr("Speed"), format!("{} Mb/s", dev.bitrate / 1000)),
                            ]);
                        }
                    } else {
                        pairs.push((tr("Speed"), format!("{} Mb/s", dev.speed)));
                    }
                    // O iwd só sabe a frequência e a taxa conectado, e o networkd não dá a velocidade
                    if snap.backend == Backend::Standalone {
                        let unknown = [format!("0 MHz ({})", nm::freq_band(0)), "0 Mb/s".to_string()];
                        pairs.retain(|(_, v)| !unknown.contains(v));
                    }
                    (active, pairs)
                }
            },
        };
        let Some(active) = active else {
            self.show_list(); // a conexão caiu com a tela aberta
            return;
        };
        let ip4 = active.ip4.as_deref().and_then(|p| snap.ip.get(p)).cloned().unwrap_or_default();
        pairs.extend(ip4.addresses.iter().map(|a| ("IPv4".to_string(), a.clone())));
        if !ip4.gateway.is_empty() {
            pairs.push(("Gateway".into(), ip4.gateway.clone()));
        }
        if !ip4.dns.is_empty() {
            pairs.push(("DNS".into(), ip4.dns.join(", ")));
        }
        let ip6 = active.ip6.as_deref().and_then(|p| snap.ip.get(p)).cloned().unwrap_or_default();
        pairs.extend(ip6.addresses.iter().map(|a| ("IPv6".to_string(), a.clone())));
        if !ip6.gateway.is_empty() {
            pairs.push(("Gateway v6".into(), ip6.gateway.clone()));
        }

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
            v.set_focusable(false); // sem isso o texto aparece todo selecionado
            v.set_wrap(true);
            v.set_wrap_mode(pango::WrapMode::Char);
            v.set_max_width_chars(30);
            v.set_hexpand(true);
            grid.attach(&k, 0, i as i32, 1, 1);
            grid.attach(&v, 1, i as i32, 1, 1);
        }
        self.info.append(&grid);
        self.popup.rendered_at.set(Instant::now());
    }

    // --- ações ---
    fn connect_wifi(self: &Rc<Self>, dev_path: &str, ssid: &str) {
        // A lista só é refeita quando algo visível muda; o ponto de acesso desta linha pode ter saído do
        // alcance enquanto outro da mesma rede ficou
        let snap = self.snap.borrow().clone();
        let Some(dev) = snap.device(dev_path) else { return };
        let Some(r) = nm::group_networks(&snap, dev).into_iter().find(|n| n.ssid == ssid) else {
            self.set_status(&tr("%s is out of range").replace("%s", ssid));
            self.schedule();
            return;
        };
        self.activation_name.replace(ssid.to_string());
        if snap.backend == Backend::Standalone {
            if nm::security_of(r.ap).1 && r.saved.is_empty() {
                // O iwd só conecta a uma 802.1X com o arquivo de provisionamento dela (/var/lib/iwd)
                self.set_status(&tr("Enterprise networks are not supported"));
            } else {
                self.iwd_connect(&r.ap.path, ssid);
            }
            return;
        }
        if let Some(saved) = r.saved.first() {
            self.nm_activate(Some(&saved.path), Some(dev_path), Some(&r.ap.path));
        } else if nm::security_of(r.ap).1 {
            // O NM não completa um perfil 802.1X sozinho
            self.open_editor(&["--create", "--type=802-11-wireless"]);
        } else {
            self.nm_add_activate(dev_path, &r.ap.path);
        }
    }

    fn object_path(path: Option<&str>) -> glib::Variant {
        glib::variant::ObjectPath::try_from(path.unwrap_or("/").to_string()).unwrap_or_else(|_| glib::variant::ObjectPath::try_from("/".to_string()).unwrap()).to_variant()
    }

    fn nm_call(self: &Rc<Self>, path: &str, iface: &str, method: &str, args: glib::Variant, done: impl FnOnce(&Rc<Popup>, Result<glib::Variant, glib::Error>) + 'static) {
        let Some(bus) = self.bus.clone() else { return };
        let me = Rc::downgrade(self);
        bus.call(Some(nm::NM), path, iface, method, Some(&args), None, gio::DBusCallFlags::NONE, CALL_MS, gio::Cancellable::NONE, move |result| {
            if let Some(p) = me.upgrade() {
                done(&p, result);
            }
        });
    }

    fn nm_activate(self: &Rc<Self>, conn: Option<&str>, dev: Option<&str>, ap: Option<&str>) {
        self.set_status("");
        self.activation.replace(None);
        let args = glib::Variant::tuple_from_iter([Self::object_path(conn), Self::object_path(dev), Self::object_path(ap)]);
        self.nm_call(nm::NM_PATH, nm::NM, "ActivateConnection", args, |p, result| p.finish_activation(result.map(|v| v.child_value(0))));
    }

    /// Perfil vazio: o NM completa a partir do ponto de acesso e, se faltar senha, pergunta ao agente de
    /// segredos (a janela do nm-applet).
    fn nm_add_activate(self: &Rc<Self>, dev: &str, ap: &str) {
        self.set_status("");
        self.activation.replace(None);
        let empty = glib::Variant::array_from_iter_with_type(glib::VariantTy::new("{sa{sv}}").expect("tipo"), std::iter::empty::<glib::Variant>());
        let args = glib::Variant::tuple_from_iter([empty, Self::object_path(Some(dev)), Self::object_path(Some(ap))]);
        self.nm_call(nm::NM_PATH, nm::NM, "AddAndActivateConnection", args, |p, result| p.finish_activation(result.map(|v| v.child_value(1))));
    }

    /// Desliga o dispositivo, pelo NM ou pelo iwd.
    fn disconnect(self: &Rc<Self>, dev: &str) {
        if self.snap.borrow().backend == Backend::Standalone {
            self.set_status("");
            self.agent.cancel();
            self.iwd_call(dev, iwd::IWD_STATION, "Disconnect", None, CALL_MS, |p, result| {
                if let Err(err) = result {
                    p.set_status(&tr("Could not disconnect: %s").replace("%s", &error_text(&err)));
                }
            });
        } else {
            self.nm_disconnect(dev);
        }
    }

    fn iwd_call(self: &Rc<Self>, path: &str, iface: &str, method: &str, args: Option<glib::Variant>, timeout: i32, done: impl FnOnce(&Rc<Popup>, Result<glib::Variant, glib::Error>) + 'static) {
        let Some(bus) = self.bus.clone() else { return };
        let me = Rc::downgrade(self);
        bus.call(Some(iwd::IWD), path, iface, method, args.as_ref(), None, gio::DBusCallFlags::NO_AUTO_START, timeout, gio::Cancellable::NONE, move |result| {
            if let Some(p) = me.upgrade() {
                done(&p, result);
            }
        });
    }

    /// O `Network.Connect` só volta depois de conectar ou falhar, inclusive com o usuário digitando a senha:
    /// sem limite de tempo. O iwd pergunta a senha de uma rede nova ao agente (este popup).
    fn iwd_connect(self: &Rc<Self>, network: &str, ssid: &str) {
        self.set_status("");
        self.agent.take_answered();
        let name = ssid.to_string();
        self.iwd_call(network, iwd::IWD_NETWORK, "Connect", None, i32::MAX, move |p, result| {
            let answered = p.agent.take_answered();
            let Err(err) = result else { return };
            // Cancelado aqui (Esc, o botão, outra rede, desconectar) não é falha
            let remote = gio::DBusError::remote_error(&err).map(|n| n.to_string()).unwrap_or_default();
            if matches!(remote.as_str(), "net.connman.iwd.Aborted" | "net.connman.iwd.Agent.Error.Canceled") {
                return;
            }
            // O iwd devolve um "Failed" genérico, em inglês: com a senha dada agora, quase sempre é ela
            if remote == "net.connman.iwd.Failed" {
                let text = if answered { tr("Could not connect to %s. Check the password.") } else { tr("Could not connect to %s") };
                p.set_status(&text.replace("%s", &name));
                return;
            }
            let text = if remote == "net.connman.iwd.NoAgent" {
                tr("Another password agent is active")
            } else {
                error_text(&err)
            };
            p.set_status(&tr("Could not connect to %s").replace("%s", &format!("{name}: {text}")));
        });
    }

    /// O interruptor do Wi-Fi sem o NM: o Powered de cada placa do iwd.
    fn iwd_power(self: &Rc<Self>, on: bool) {
        let devices: Vec<String> = nm::wifi_devices(&self.snap.borrow()).iter().map(|d| d.path.clone()).collect();
        for dev in devices {
            let args = glib::Variant::tuple_from_iter([iwd::IWD_DEVICE.to_variant(), "Powered".to_variant(), glib::Variant::from_variant(&on.to_variant())]);
            self.iwd_call(&dev, "org.freedesktop.DBus.Properties", "Set", Some(args), CALL_MS, |p, result| {
                if let Err(err) = result {
                    p.set_status(&error_text(&err));
                    p.schedule(); // o interruptor volta ao que o iwd diz
                }
            });
        }
    }

    /// O que o agente avisa: a senha pedida (mostra o campo), a resposta dada ou cancelada (volta à lista)
    /// e os pedidos que não sabe atender.
    fn on_agent(self: &Rc<Self>, event: agent::Event) {
        match event {
            agent::Event::Asking(network) => {
                let ssid = self.snap.borrow().access_points.get(&network).map(|a| a.ssid.clone()).unwrap_or_default();
                self.agent.ask(&ssid);
                self.detail.replace(None);
                self.stack.set_visible_child_name("password");
                self.back.set_visible(false);
                self.popup.rendered_at.set(Instant::now());
            }
            agent::Event::Done => {
                if self.stack.visible_child_name().as_deref() == Some("password") {
                    self.show_list();
                }
            }
            agent::Event::Enterprise => self.set_status(&tr("Enterprise networks are not supported")),
            agent::Event::State(AgentState::Taken) => self.set_status(&tr("Another password agent is active")),
            agent::Event::State(_) => {}
            agent::Event::Failed(text) => self.set_status(&text),
        }
    }

    fn nm_disconnect(self: &Rc<Self>, dev: &str) {
        self.set_status("");
        self.activation.replace(None); // cancelar não é falha
        self.nm_call(dev, "org.freedesktop.NetworkManager.Device", "Disconnect", glib::Variant::tuple_from_iter(std::iter::empty::<glib::Variant>()), |p, result| {
            if let Err(err) = result {
                p.set_status(&tr("Could not disconnect: %s").replace("%s", err.message()));
            }
        });
    }

    fn nm_deactivate(self: &Rc<Self>, active: &str) {
        self.set_status("");
        self.activation.replace(None); // cancelar não é falha
        self.nm_call(nm::NM_PATH, nm::NM, "DeactivateConnection", glib::Variant::tuple_from_iter([Self::object_path(Some(active))]), |p, result| {
            if let Err(err) = result {
                p.set_status(&tr("Could not disconnect: %s").replace("%s", err.message()));
            }
        });
    }

    fn finish_activation(self: &Rc<Self>, result: Result<glib::Variant, glib::Error>) {
        match result {
            Err(err) => self.set_status(&tr("Could not connect: %s").replace("%s", &error_text(&err))),
            Ok(path) => {
                // Só conta como falha se esta ativação cair antes de conectar
                let Some(path) = path.str().map(String::from) else { return };
                self.activation.replace(Some(path.clone()));
                self.activation_at.set(Some(Instant::now()));
                self.schedule(); // a próxima leitura confere se ela ainda existe
                // O aviso pode ter chegado antes desta resposta
                let seen = self.active_states.borrow().get(&path).copied();
                if let Some((state, reason)) = seen {
                    self.on_active_state(&path, state, reason);
                }
            }
        }
    }

    fn on_active_state(self: &Rc<Self>, path: &str, state: u32, reason: u32) {
        self.active_states.borrow_mut().insert(path.to_string(), (state, reason));
        self.schedule();
        if self.activation.borrow().as_deref() != Some(path) {
            return;
        }
        if state == active_state::ACTIVATED {
            self.activation.replace(None);
        } else if state == active_state::DEACTIVATED {
            self.activation.replace(None);
            if reason != active_state::REASON_USER_DISCONNECTED {
                let id = self.snap.borrow().active(Some(path)).map(|a| a.id.clone()).unwrap_or_else(|| self.activation_name.borrow().clone());
                self.set_status(&tr("Could not connect to %s").replace("%s", &id));
            }
        }
    }

    fn open_editor(&self, args: &[&str]) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        if spawn_detached("nm-connection-editor", &args).is_ok() {
            self.popup.close("editor");
        } else {
            self.spawn("nm-connection-editor", &args);
        }
    }
}

/// Avisa o ícone (waybar-network), que esconde a dica com o popup aberto.
fn notify_icon() {
    resident::signal_pid(&resident::pidfile("waybar-network"), "waybar-network", libc::SIGUSR1);
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-network-popup em Python confere que o binário carrega
    }
    run_popup(PROGRAM, |main_loop| Some(Popup::new(main_loop)), notify_icon)
}
