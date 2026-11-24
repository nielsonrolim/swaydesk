//! Daemon de notificações: é dono de org.freedesktop.Notifications e guarda a lista em memória.
//! O porte de `home/.local/bin/sway-notifd`, que passa a vez a este binário (sway_rust).
//!
//! Mostra os toasts e atende o popup e o ícone da Waybar por uma interface de controle no mesmo nome e
//! caminho. Sem disco: a lista acaba com o processo, e código ou mensagem não ficam gravados.
//!
//! --headless: sem janela, para os testes. --check: só confere que o binário carrega.
mod toasts;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk4::{gdk_pixbuf, gio, glib};
use gtk4::gio::prelude::*;
use swaydesk_common::resident;
use swaydesk_notifd_logic::{self as wn, Center, Dnd, Hint, Hints, Incoming, Notification, ToastQueue};

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const IFACE: &str = "org.freedesktop.Notifications";
const CONTROL: &str = "local.SwayDesktop.Notifications1";
const PROGRAM: &str = "sway-notifd";
const POPUP: &str = "waybar-notifications-popup";
const IMAGE_SIZE: i32 = 128;
/// Fica em tmpfs, ou seja, na memória
const LOG_MAX_BYTES: u64 = 256 * 1024;

const CAPABILITIES: &[&str] = &[
    "actions", "body", "body-markup", "icon-static", "persistence", "synchronous", "private-synchronous",
    "x-canonical-private-synchronous",
];
/// Da mais nova para a mais antiga. O icon_data (da especificação 1.0) fica por último na ordem dos
/// ícones: depois do image-path e do app_icon
const IMAGE_HINTS: [&str; 3] = ["image-data", "image_data", "icon_data"];
const LEGACY_IMAGE: &str = "icon_data";
const IMAGE_TYPE: &str = "(iiibiiay)";
/// Mais os de wn::SYNC_HINTS
const KNOWN_HINTS: &[&str] = &["urgency", "transient", "resident", "desktop-entry", "image-path", "image_path"];

fn known_hint(key: &str) -> bool {
    KNOWN_HINTS.contains(&key) || wn::SYNC_HINTS.contains(&key)
}

/// Roda um callback do GLib ou do GTK sem deixar um pânico atravessar o trampolim em C, o que abortaria
/// o daemon (e soltaria o nome no barramento): registra e segue, como o Python com o traceback.
pub fn guard<R>(name: &str, f: impl FnOnce() -> R) -> Option<R> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(_) => {
            log(&format!("{name}: panic"));
            None
        }
    }
}

const XML: &str = r#"
<node>
  <interface name="org.freedesktop.Notifications">
    <method name="GetCapabilities"><arg type="as" direction="out"/></method>
    <method name="Notify">
      <arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/>
      <arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/>
      <arg type="a{sv}" direction="in"/><arg type="i" direction="in"/>
      <arg type="u" direction="out"/>
    </method>
    <method name="CloseNotification"><arg type="u" direction="in"/></method>
    <method name="GetServerInformation">
      <arg type="s" direction="out"/><arg type="s" direction="out"/>
      <arg type="s" direction="out"/><arg type="s" direction="out"/>
    </method>
    <signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
    <signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
  </interface>
  <interface name="local.SwayDesktop.Notifications1">
    <method name="List"><arg type="s" direction="out"/></method>
    <method name="Dismiss"><arg type="u" direction="in"/></method>
    <method name="DismissApp"><arg type="s" direction="in"/></method>
    <method name="ClearAll"/>
    <method name="Invoke"><arg type="u" direction="in"/><arg type="s" direction="in"/></method>
    <method name="SetDnd"><arg type="s" direction="in"/></method>
    <method name="HideToasts"/>
    <method name="CenterOpened"><arg name="width" type="u" direction="in"/></method>
    <signal name="Changed"/>
  </interface>
</node>
"#;

fn log_path() -> PathBuf {
    resident::runtime_dir().join("sway-notifd.log")
}

fn image_dir() -> PathBuf {
    resident::runtime_dir().join("sway-notifd")
}

fn image_file(id: u32) -> PathBuf {
    image_dir().join(format!("{id}.png"))
}

/// O programa dono do nome: o comm do processo que o barramento diz ser o dono.
fn name_owner(conn: &gio::DBusConnection) -> Option<String> {
    let call = |method: &str, arg: &str| {
        conn.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            method,
            Some(&(arg,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
        )
        .ok()
    };
    let (unique,) = call("GetNameOwner", NAME)?.get::<(String,)>()?;
    let (pid,) = call("GetConnectionUnixProcessID", &unique)?.get::<(u32,)>()?;
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    // O comm é texto livre do outro processo: caracteres de controle viram "?" antes do terminal e do log
    let comm: String = comm.trim().chars().map(|c| if c.is_control() { '?' } else { c }).collect();
    Some(format!("{comm} (pid {pid})"))
}

/// O aviso de quando outro programa tem o nome: o pedido fica na fila, e o nome vem quando ele sair.
fn waiting_message(owner: Option<&str>) -> String {
    format!(
        "org.freedesktop.Notifications is owned by {}; waiting: notifications go there until it exits",
        owner.unwrap_or("another program")
    )
}

/// Só o que deu errado, nunca o conteúdo das notificações.
fn log(message: &str) {
    let now = glib::DateTime::now_local().ok().and_then(|t| t.format("%F %T").ok()).unwrap_or_default();
    let line = format!("{now} {message}\n");
    let path = log_path();
    // Um programa pode provocar erros à vontade: passou do limite, recomeça do zero
    let full = std::fs::metadata(&path).is_ok_and(|m| m.len() + line.len() as u64 > LOG_MAX_BYTES);
    let mut options = std::fs::OpenOptions::new();
    options.create(true);
    if full {
        options.write(true).truncate(true);
    } else {
        options.append(true);
    }
    if let Ok(mut file) = options.open(&path) {
        let bytes = line.as_bytes();
        let _ = file.write_all(&bytes[bytes.len().saturating_sub(LOG_MAX_BYTES as usize)..]);
    }
}

/// Um valor de dica como o `unpack()` do Python o veria: números, booleanos e textos.
fn hint_value(value: &glib::Variant) -> Hint {
    match value.type_().as_str() {
        "b" => Hint::Bool(value.get::<bool>().unwrap_or(false)),
        "y" => Hint::Int(value.get::<u8>().unwrap_or(0) as i64),
        "n" => Hint::Int(value.get::<i16>().unwrap_or(0) as i64),
        "q" => Hint::Int(value.get::<u16>().unwrap_or(0) as i64),
        "i" => Hint::Int(value.get::<i32>().unwrap_or(0) as i64),
        "u" => Hint::Int(value.get::<u32>().unwrap_or(0) as i64),
        "x" => Hint::Int(value.get::<i64>().unwrap_or(0)),
        "t" => Hint::Int(value.get::<u64>().map_or(i64::MAX, |v| i64::try_from(v).unwrap_or(i64::MAX))),
        "s" | "o" | "g" => Hint::Str(value.str().unwrap_or("").to_string()),
        _ => Hint::Other(truthy(value)),
    }
}

/// O `bool()` do Python no valor desempacotado: zero, texto e coleções vazias são falsos.
fn truthy(value: &glib::Variant) -> bool {
    match value.type_().as_str() {
        "d" => value.get::<f64>().is_some_and(|d| d != 0.0),
        "h" => value.get::<glib::variant::Handle>().is_some_and(|h| h.0 != 0),
        "v" => value.as_variant().is_some_and(|inner| hint_value(&inner).truthy()),
        _ if value.is_container() => value.n_children() > 0,
        _ => true,
    }
}

/// Só as dicas conhecidas, e as imagens sem desempacotar os bytes: a do image-data (ou a do image_data)
/// e a antiga, do icon_data.
fn read_hints(variant: &glib::Variant) -> (Hints, Option<glib::Variant>, Option<glib::Variant>) {
    let mut hints = Hints::new();
    let mut images: HashMap<&str, glib::Variant> = HashMap::new();
    for i in 0..variant.n_children() {
        let entry = variant.child_value(i);
        let key = entry.child_value(0);
        let Some(key) = key.str() else { continue };
        let Some(value) = entry.child_value(1).as_variant() else { continue };
        if let Some(name) = IMAGE_HINTS.iter().find(|n| **n == key) {
            if value.type_().as_str() == IMAGE_TYPE {
                images.insert(name, value);
            }
        } else if known_hint(key) {
            hints.insert(key.to_string(), hint_value(&value));
        }
    }
    let legacy = images.remove(LEGACY_IMAGE);
    let image = IMAGE_HINTS.iter().find_map(|k| images.remove(k));
    (hints, image, legacy)
}

fn hint_str<'a>(hints: &'a Hints, name: &str) -> &'a str {
    match hints.get(name) {
        Some(Hint::Str(s)) => s,
        _ => "",
    }
}

/// Ícone do .desktop indicado pelo app; o nome vem de fora, e não pode ser um caminho.
fn entry_icon(entry: &str) -> String {
    if entry.is_empty() || entry.contains('/') {
        return String::new();
    }
    gio::DesktopAppInfo::new(&format!("{entry}.desktop"))
        .and_then(|info| info.icon())
        .and_then(|icon| IconExt::to_string(&icon))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Só do usuário. Se já houver outra coisa no lugar, segue sem imagens em vez de confiar nela.
fn prepare_image_dir() -> bool {
    let dir = image_dir();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&dir); // um link ou arquivo no lugar
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => true,
        Err(err) => {
            log(&format!("no image folder: {err}"));
            false
        }
    }
}

/// Grava image-data como PNG; devolve o caminho, ou None se a imagem não prestar.
fn save_image(variant: &glib::Variant, id: u32) -> Option<String> {
    let int = |i| variant.child_value(i).get::<i32>().unwrap_or(0) as i64;
    let (width, height, rowstride) = (int(0), int(1), int(2));
    let has_alpha = variant.child_value(3).get::<bool>().unwrap_or(false);
    let (bits, channels) = (int(4), int(5));
    let data = variant.child_value(6).data_as_bytes();
    if !wn::valid_image(width, height, rowstride, has_alpha, bits, channels, data.len() as i64) {
        return None;
    }
    let mut bytes = data.to_vec();
    bytes.resize((rowstride * height) as usize, 0); // a última linha pode vir sem a sobra
    let pixbuf = gdk_pixbuf::Pixbuf::from_bytes(
        &glib::Bytes::from_owned(bytes),
        gdk_pixbuf::Colorspace::Rgb,
        has_alpha,
        8,
        width as i32,
        height as i32,
        rowstride as i32,
    );
    let scale = (IMAGE_SIZE as f64 / width as f64).min(IMAGE_SIZE as f64 / height as f64).min(1.0);
    let pixbuf = if scale < 1.0 {
        let w = ((width as f64 * scale) as i32).max(1);
        let h = ((height as f64 * scale) as i32).max(1);
        pixbuf.scale_simple(w, h, gdk_pixbuf::InterpType::Bilinear)?
    } else {
        pixbuf
    };
    let path = image_file(id);
    let tmp = path.with_extension("png.tmp");
    let saved = pixbuf.savev(&tmp, "png", &[]).map_err(|e| e.to_string()).and_then(|()| std::fs::rename(&tmp, &path).map_err(|e| e.to_string()));
    match saved {
        Ok(()) => Some(path.to_string_lossy().into_owned()),
        Err(err) => {
            log(&format!("image not saved: {err}"));
            let _ = std::fs::remove_file(&tmp);
            None
        }
    }
}

fn popup_open() -> bool {
    resident::live_pid(&resident::pidfile(POPUP), POPUP).is_some()
}

/// A tela dos toasts: a janela, ou nada no --headless (a fila e os tempos funcionam do mesmo jeito).
pub trait View {
    fn render(&self, items: &[&Notification]);
    /// Afasta os toasts da borda direita (a central aberta ocupa o canto); 0 volta ao canto.
    fn set_right_offset(&self, _offset: i32) {}
}

/// O espaço entre a central e os toasts afastados para a esquerda dela.
const CENTER_GAP: i32 = 8;
/// De quanto em quanto tempo se confere se a central fechou, com os toasts afastados.
const CENTER_WATCH_MS: u64 = 300;

struct NoToasts;

impl View for NoToasts {
    fn render(&self, _items: &[&Notification]) {}
}

/// Um erro que vai de volta a quem chamou.
enum CallError {
    InvalidArgs(String),
}

pub struct Service {
    /// Para os temporizadores, que guardam o serviço sem mantê-lo vivo
    me: std::rc::Weak<Service>,
    center: RefCell<Center>,
    dnd: RefCell<Dnd>,
    queue: RefCell<ToastQueue>,
    /// id -> (fonte do GLib, revisão)
    timers: RefCell<HashMap<u32, (glib::SourceId, u64)>>,
    dnd_timer: RefCell<Option<glib::SourceId>>,
    conn: RefCell<Option<gio::DBusConnection>>,
    images: Cell<bool>,
    owned: Cell<bool>,
    view: RefCell<Option<Box<dyn View>>>,
    /// Com a central aberta, os toasts (só as críticas) vão para a esquerda dela; a fonte confere quando
    /// ela fecha, ou cai, para voltarem ao canto
    center_watch: RefCell<Option<glib::SourceId>>,
    /// A notificação do Notify em andamento, para o reparo depois de um pânico
    creating: Cell<Option<u32>>,
}

impl Service {
    fn new() -> Rc<Service> {
        Rc::new_cyclic(|me| Service {
            me: me.clone(),
            center: RefCell::new(Center::new(wn::system_clock(), wn::MAX_ITEMS)),
            dnd: RefCell::new(Dnd::new(wn::system_clock())),
            queue: RefCell::new(ToastQueue::new(wn::MAX_TOASTS)),
            timers: RefCell::new(HashMap::new()),
            dnd_timer: RefCell::new(None),
            conn: RefCell::new(None),
            images: Cell::new(false),
            owned: Cell::new(false),
            view: RefCell::new(None),
            center_watch: RefCell::new(None),
            creating: Cell::new(None),
        })
    }

    // --- barramento ---
    fn on_bus_acquired(&self, conn: gio::DBusConnection) {
        *self.conn.borrow_mut() = Some(conn.clone());
        let node = match gio::DBusNodeInfo::for_xml(XML) {
            Ok(node) => node,
            Err(err) => {
                log(&format!("bad interface XML: {err}"));
                return;
            }
        };
        for iface in node.interfaces() {
            let weak = self.me.clone();
            let registered = conn
                .register_object(PATH, iface)
                .method_call(move |_conn, _sender, _path, _iface, method, params, invocation| {
                    if let Some(service) = weak.upgrade() {
                        service.on_call(method, &params, invocation);
                    }
                })
                .build();
            if let Err(err) = registered {
                log(&format!("register object: {err}"));
            }
        }
    }

    /// Só agora a pasta é deste processo: quem perde a disputa não pode apagar a do dono.
    fn on_name_acquired(&self) {
        self.owned.set(true);
        self.images.set(prepare_image_dir());
    }

    fn on_call(&self, method: &str, params: &glib::Variant, invocation: gio::DBusMethodInvocation) {
        // Um pedido ruim não pode derrubar as notificações
        match guard(method, || self.dispatch(method, params)) {
            Some(Ok(value)) => invocation.return_value(value.as_ref()),
            Some(Err(CallError::InvalidArgs(message))) => {
                invocation.return_dbus_error("org.freedesktop.DBus.Error.InvalidArgs", &message)
            }
            None => {
                guard("repair", || self.repair());
                invocation.return_dbus_error("org.freedesktop.DBus.Error.Failed", "internal error");
            }
        }
    }

    /// Depois de um pânico no meio de um pedido: a notificação que estava sendo criada sai (quem pediu
    /// não recebeu o id), e a tela e os tempos voltam a seguir a lista.
    fn repair(&self) {
        if let Some(id) = self.creating.take() {
            if let Ok(mut center) = self.center.try_borrow_mut() {
                center.remove(id);
            }
            self.drop_item(id, wn::UNDEFINED);
        }
        if let Ok(mut queue) = self.queue.try_borrow_mut() {
            if let Ok(center) = self.center.try_borrow() {
                let gone: Vec<u32> = queue.ids().iter().copied().filter(|id| center.get(*id).is_none()).collect();
                for id in gone {
                    queue.hide(id);
                }
            }
        }
        self.sync_toasts();
        self.changed();
    }

    fn dispatch(&self, method: &str, params: &glib::Variant) -> Result<Option<glib::Variant>, CallError> {
        let u32_at = |i| params.child_value(i).get::<u32>().unwrap_or(0);
        let str_at = |i| params.child_value(i).str().unwrap_or("").to_string();
        match method {
            "GetCapabilities" => {
                let caps: Vec<String> = CAPABILITIES.iter().map(|s| s.to_string()).collect();
                return Ok(Some((caps,).to_variant()));
            }
            "GetServerInformation" => return Ok(Some(("sway-notifd", "sway-desktop", "1.0", "1.2").to_variant())),
            "Notify" => return Ok(Some((self.notify(params),).to_variant())),
            "CloseNotification" => self.remove(u32_at(0), wn::CLOSED),
            "List" => {
                let snap = wn::snapshot(&self.center.borrow(), &mut self.dnd.borrow_mut());
                return Ok(Some((snap.to_string(),).to_variant()));
            }
            "Dismiss" => self.remove(u32_at(0), wn::DISMISSED),
            "DismissApp" => {
                let removed = self.center.borrow_mut().remove_app(&str_at(0));
                self.remove_many(removed);
            }
            "ClearAll" => {
                let removed = self.center.borrow_mut().clear();
                self.remove_many(removed);
            }
            "Invoke" => self.invoke(u32_at(0), &str_at(1)),
            "SetDnd" => self.set_dnd(&str_at(0))?,
            "HideToasts" => self.hide_for_center(),
            "CenterOpened" => {
                self.hide_for_center();
                self.watch_center(u32_at(0).min(4000) as i32);
            }
            _ => {}
        }
        Ok(None)
    }

    fn emit(&self, iface: &str, name: &str, value: Option<glib::Variant>) {
        let conn = self.conn.borrow().clone();
        let Some(conn) = conn.filter(|c| !c.is_closed()) else { return };
        if let Err(err) = conn.emit_signal(None, PATH, iface, name, value.as_ref()) {
            log(&format!("signal {name}: {err}"));
        }
    }

    fn changed(&self) {
        self.emit(CONTROL, "Changed", None);
    }

    fn closed(&self, id: u32, reason: u32) {
        self.emit(IFACE, "NotificationClosed", Some((id, reason).to_variant()));
    }

    // --- interface padrão ---
    fn notify(&self, params: &glib::Variant) -> u32 {
        let (hints, image, legacy) = read_hints(&params.child_value(6));
        let app_icon = params.child_value(2).str().unwrap_or("").to_string();
        let actions: Vec<String> = params.child_value(5).get::<Vec<String>>().unwrap_or_default();
        let incoming = Incoming {
            app_name: params.child_value(0).str().unwrap_or("").to_string(),
            replaces_id: params.child_value(1).get::<u32>().unwrap_or(0),
            icon: String::new(),
            summary: params.child_value(3).str().unwrap_or("").to_string(),
            body: params.child_value(4).str().unwrap_or("").to_string(),
            actions,
            hints: hints.clone(),
            expire_timeout: params.child_value(7).get::<i32>().unwrap_or(-1),
        };
        let (item, evicted) = self.center.borrow_mut().add(incoming);
        self.creating.set(Some(item.id));
        // A notificação já está na lista: daqui em diante, nada pode interromper o Notify
        for old in &evicted {
            self.drop_item(old.id, wn::UNDEFINED);
        }
        let image_path = match hint_str(&hints, "image-path") {
            "" => hint_str(&hints, "image_path"),
            path => path,
        };
        // A ordem da especificação: image-data, image-path, app_icon e, por último, o icon_data
        let image = image.or(if image_path.is_empty() && app_icon.is_empty() { legacy } else { None });
        let saved = self.try_save_image(image.as_ref(), item.id);
        if saved.is_none() {
            forget_image(item.id); // substituiu uma que tinha imagem
        }
        let icon = wn::icon_spec(
            saved.as_deref().unwrap_or(""),
            image_path,
            &app_icon,
            &entry_icon(hint_str(&hints, "desktop-entry")),
        );
        if let Some(stored) = self.center.borrow_mut().get_mut(item.id) {
            stored.icon = icon;
        }
        let dnd_active = self.dnd.borrow_mut().active();
        if wn::should_toast(&item, dnd_active, popup_open()) {
            if item.urgency == wn::CRITICAL {
                let displaced = self.queue.borrow_mut().show_urgent(item.id);
                if let Some(id) = displaced {
                    self.end_transient(id); // saiu da tela: uma transitória sai também da lista
                }
            } else {
                self.queue.borrow_mut().show(item.id);
            }
        } else if item.transient {
            // Ninguém a veria, e ela não fica na lista
            self.center.borrow_mut().remove(item.id);
            self.queue.borrow_mut().hide(item.id); // substituiu uma que estava na tela
            // Depois da resposta: antes dela, o app ainda não sabe de quem é o aviso
            let weak = self.me.clone();
            let id = item.id;
            glib::idle_add_local_once(move || {
                if let Some(service) = weak.upgrade() {
                    guard("close unseen", || service.drop_item(id, wn::EXPIRED));
                }
            });
        } else {
            self.queue.borrow_mut().hide(item.id); // substituiu uma que estava na tela
        }
        self.sync_toasts();
        self.changed(); // mesmo uma transitória pode ter tirado itens da lista, ao substituir ou expulsar
        self.creating.set(None);
        item.id
    }

    fn try_save_image(&self, image: Option<&glib::Variant>, id: u32) -> Option<String> {
        let image = image?;
        if !self.images.get() {
            return None;
        }
        // Sem a imagem, a notificação ainda serve
        catch_unwind(AssertUnwindSafe(|| save_image(image, id))).unwrap_or_else(|_| {
            log("image not saved: panic");
            None
        })
    }

    fn set_dnd(&self, choice: &str) -> Result<(), CallError> {
        self.dnd.borrow_mut().set(choice).map_err(CallError::InvalidArgs)?;
        if let Some(timer) = self.dnd_timer.borrow_mut().take() {
            timer.remove();
        }
        if self.dnd.borrow().until.is_some() {
            let weak = self.me.clone();
            let timer = glib::timeout_add_seconds_local(30, move || {
                let Some(service) = weak.upgrade() else { return glib::ControlFlow::Break };
                guard("dnd timer", || {
                    if service.dnd.borrow_mut().active() {
                        return glib::ControlFlow::Continue;
                    }
                    service.dnd_timer.borrow_mut().take();
                    service.changed();
                    glib::ControlFlow::Break
                })
                .unwrap_or(glib::ControlFlow::Continue)
            });
            *self.dnd_timer.borrow_mut() = Some(timer);
        }
        self.changed();
        Ok(())
    }

    // --- ações de quem está na tela ---
    pub fn invoke(&self, id: u32, key: &str) {
        let resident = match self.center.borrow().get(id) {
            Some(item) if item.has_action(key) => item.resident,
            _ => return, // o app ou a lista já a fechou
        };
        self.emit(IFACE, "ActionInvoked", Some((id, key).to_variant()));
        if resident {
            self.hide_toast(id); // continua na lista; na tela, outro clique repetiria a ação
        } else {
            self.remove(id, wn::DISMISSED);
        }
    }

    /// Clique no toast: abre o app, se ele oferecer isso; senão só tira da tela.
    pub fn on_toast_body(&self, id: u32) {
        let has_default = self.center.borrow().get(id).is_some_and(Notification::has_default);
        if has_default {
            self.invoke(id, "default");
        } else {
            self.hide_toast(id);
        }
    }

    pub fn on_toast_close(&self, id: u32) {
        self.remove(id, wn::DISMISSED);
    }

    // --- saídas ---
    fn remove(&self, id: u32, reason: u32) {
        let Some(item) = self.center.borrow_mut().remove(id) else { return };
        self.drop_item(item.id, reason);
        self.sync_toasts();
        if !item.transient {
            self.changed();
        }
    }

    fn remove_many(&self, items: Vec<Notification>) {
        for item in &items {
            self.drop_item(item.id, wn::DISMISSED);
        }
        self.sync_toasts();
        self.changed();
    }

    /// A notificação já saiu da lista: limpa o resto e avisa o app.
    fn drop_item(&self, id: u32, reason: u32) {
        self.queue.borrow_mut().hide(id);
        forget_image(id);
        self.closed(id, reason);
    }

    fn end_transient(&self, id: u32) {
        let transient = self.center.borrow().get(id).is_some_and(|i| i.transient);
        if transient {
            self.center.borrow_mut().remove(id);
            self.drop_item(id, wn::EXPIRED);
        }
    }

    /// A central abriu: ela já mostra as outras, e as críticas ficam na tela (decisão do usuário).
    fn hide_for_center(&self) {
        let ids: Vec<u32> = {
            let center = self.center.borrow();
            let queue = self.queue.borrow();
            queue.ids().iter().copied().filter(|id| center.get(*id).is_none_or(|i| i.urgency != wn::CRITICAL)).collect()
        };
        for id in ids {
            self.queue.borrow_mut().hide(id);
            self.end_transient(id);
        }
        self.sync_toasts();
    }

    /// A central ocupa o canto dos toasts: eles vão para a esquerda dela até ela fechar. O fechamento vem
    /// do arquivo do PID, que também cobre uma central que caiu sem avisar.
    fn watch_center(&self, width: i32) {
        self.set_right_offset(width + CENTER_GAP);
        if self.center_watch.borrow().is_some() {
            return;
        }
        let weak = self.me.clone();
        let source = glib::timeout_add_local(Duration::from_millis(CENTER_WATCH_MS), move || {
            let Some(service) = weak.upgrade() else { return glib::ControlFlow::Break };
            if popup_open() {
                return glib::ControlFlow::Continue;
            }
            service.center_watch.borrow_mut().take();
            service.set_right_offset(0);
            glib::ControlFlow::Break
        });
        *self.center_watch.borrow_mut() = Some(source);
    }

    fn set_right_offset(&self, offset: i32) {
        if let Some(view) = self.view.borrow().as_ref() {
            view.set_right_offset(offset);
        }
    }

    /// Só sai da tela: continua na lista, e o app não é avisado.
    fn hide_toast(&self, id: u32) {
        self.queue.borrow_mut().hide(id);
        self.end_transient(id);
        self.sync_toasts();
    }

    // --- tempos ---
    fn sync_toasts(&self) {
        let center = self.center.borrow();
        let queue = self.queue.borrow();
        let visible: Vec<&Notification> = queue.visible().iter().filter_map(|id| center.get(*id)).collect();
        let mut wanted: HashMap<u32, (i64, u64)> =
            visible.iter().filter(|i| i.timeout_ms > 0).map(|i| (i.id, (i.timeout_ms, i.revision))).collect();
        // O tempo pedido pelo app corre mesmo sem toast (não perturbe, popup aberto), mas não na fila:
        // esperando vaga, ela ainda não apareceu
        let queued = queue.ids();
        for item in center.items() {
            if item.app_timeout && !queued.contains(&item.id) {
                wanted.insert(item.id, (item.timeout_ms, item.revision));
            }
        }
        drop(queue);
        let mut timers = self.timers.borrow_mut();
        let stale: Vec<u32> =
            timers.iter().filter(|(id, (_, revision))| wanted.get(id).is_none_or(|(_, r)| r != revision)).map(|(id, _)| *id).collect();
        for id in stale {
            if let Some((source, _)) = timers.remove(&id) {
                source.remove();
            }
        }
        for (id, (ms, revision)) in wanted {
            if let std::collections::hash_map::Entry::Vacant(slot) = timers.entry(id) {
                // Conta a partir de quando aparece, não de quando chegou
                let weak = self.me.clone();
                let source = glib::timeout_add_local_once(Duration::from_millis(ms as u64), move || {
                    if let Some(service) = weak.upgrade() {
                        guard("timer", || service.on_expired(id));
                    }
                });
                slot.insert((source, revision));
            }
        }
        drop(timers);
        // A lista fica emprestada durante o render: os cliques dos toasts só chegam ao serviço depois
        if let Some(view) = self.view.borrow().as_ref() {
            view.render(&visible);
        }
    }

    fn on_expired(&self, id: u32) {
        // A fonte já terminou: só sai do mapa, sem ser removida de novo
        self.timers.borrow_mut().remove(&id);
        let app_timeout = self.center.borrow().get(id).is_some_and(|i| i.app_timeout);
        if app_timeout {
            self.remove(id, wn::EXPIRED);
        } else {
            self.hide_toast(id);
        }
    }
}

fn forget_image(id: u32) {
    let _ = std::fs::remove_file(image_file(id));
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--check") {
        return ExitCode::SUCCESS; // o sway-notifd em Python confere que o binário carrega
    }
    let headless = args.iter().any(|a| a == "--headless");
    resident::set_process_name(PROGRAM);
    // Residente: nas VMs, o GL somava ~25 MB de memória suja depois dos primeiros toasts (43 contra 18 MB) e
    // ~3× a CPU de cada um; o toast não anima, e o desenho por CPU não faz falta. O notifd não abre
    // programas: nada herda a variável
    if !headless {
        swaydesk_ui::use_cairo_renderer();
    }

    let service = Service::new();
    if headless {
        *service.view.borrow_mut() = Some(Box::new(NoToasts));
    } else {
        swaydesk_common::theme_resources::register_theme_resources();
        if let Err(err) = gtk4::init() {
            eprintln!("{PROGRAM}: could not start GTK: {err}");
            return ExitCode::FAILURE;
        }
        *service.view.borrow_mut() = Some(Box::new(toasts::ToastWindow::new(Rc::downgrade(&service))));
    }

    let main_loop = glib::MainLoop::new(None, false);
    let code = Rc::new(Cell::new(0u8));
    for signal in [libc::SIGTERM, libc::SIGINT] {
        let main_loop = main_loop.clone();
        glib::unix_signal_add_local(signal, move || {
            main_loop.quit();
            glib::ControlFlow::Break
        });
    }
    let (s1, s2, s3) = (service.clone(), service.clone(), service.clone());
    let (lost_loop, lost_code) = (main_loop.clone(), code.clone());
    let _owner = gio::bus_own_name(
        gio::BusType::Session,
        NAME,
        gio::BusNameOwnerFlags::NONE,
        move |conn, _| {
            guard("bus acquired", || s1.on_bus_acquired(conn));
        },
        move |_, _| {
            guard("name acquired", || s2.on_name_acquired());
        },
        move |conn, _| {
            // Com o nome ocupado, o pedido fica na fila do barramento e o nome chega quando o outro sair.
            // Sair aqui só faria o laço do Sway reabrir este processo a cada segundo
            if let (Some(conn), false) = (conn.as_ref(), s3.owned.get()) {
                // No stderr também: fora da sessão (um Sway comum com o mako ou o dunst), quem roda à mão vê por quê
                let message = waiting_message(name_owner(conn).as_deref());
                eprintln!("{PROGRAM}: {message}");
                log(&message);
                return;
            }
            log("lost the name org.freedesktop.Notifications");
            lost_code.set(1);
            lost_loop.quit();
        },
    );
    main_loop.run();
    if service.images.get() {
        let _ = std::fs::remove_dir_all(image_dir());
    }
    ExitCode::from(code.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_waiting_message_names_the_owner() {
        // O mesmo texto do waiting_message do sway-notifd em Python
        assert_eq!(
            waiting_message(Some("mako (pid 42)")),
            "org.freedesktop.Notifications is owned by mako (pid 42); waiting: notifications go there until it exits"
        );
        assert!(waiting_message(None).contains("owned by another program; waiting"));
    }
}
