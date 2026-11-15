//! A lógica da central de notificações, sem GTK e sem D-Bus: lista, tempos, não perturbe e limpeza.
//! É o porte de `waybar_notifications.py`, que segue como a referência e o reserva.
//!
//! Tudo o que chega aqui vem de qualquer programa da sessão: é tratado como conteúdo não confiável.

pub mod body;
mod entities;

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Value};

pub use body::{clean_body, MAX_BODY};

pub const LOW: u8 = 0;
pub const NORMAL: u8 = 1;
pub const CRITICAL: u8 = 2;

/// Motivos de NotificationClosed, na especificação
pub const EXPIRED: u32 = 1;
pub const DISMISSED: u32 = 2;
pub const CLOSED: u32 = 3;
pub const UNDEFINED: u32 = 4;

pub const SYNC_HINTS: [&str; 3] = ["synchronous", "private-synchronous", "x-canonical-private-synchronous"];
pub const MAX_ITEMS: usize = 200;
pub const MAX_TOASTS: usize = 5;
pub const MAX_TEXT: usize = 300; // app, título e ícone
pub const MAX_ACTIONS: usize = 6;
pub const MAX_ACTION_LABEL: usize = 40;
pub const MAX_IMAGE_SIDE: i64 = 4096;
pub const MAX_IMAGE_BYTES: i64 = 32 * 1024 * 1024;
pub const MAX_ID: u32 = u32::MAX;

/// Quanto o toast fica na tela pela urgência; 0 não expira.
pub fn default_timeout_ms(urgency: u8) -> i64 {
    match urgency {
        LOW => 5000,
        CRITICAL => 0,
        _ => 10000,
    }
}

fn cut(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Uma linha de texto: sem caracteres de controle, quebras nem tamanho sem limite.
pub fn plain(text: &str, limit: usize) -> String {
    let clean: String = text.chars().take(limit).filter(|c| !body::is_control(*c)).collect();
    clean.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lista plana da especificação ["chave", "rótulo", ...] em pares; o último sem par é ignorado.
pub fn parse_actions(actions: &[String]) -> Vec<(String, String)> {
    actions
        .chunks_exact(2)
        .filter(|pair| !pair[0].is_empty())
        .map(|pair| (cut(&pair[0], MAX_TEXT), plain(&pair[1], MAX_ACTION_LABEL)))
        .take(MAX_ACTIONS)
        .collect()
}

/// Um valor de hint, já tirado do D-Bus.
#[derive(Clone, Debug, PartialEq)]
pub enum Hint {
    Bool(bool),
    Int(i64),
    Str(String),
    /// Outro tipo, com a verdade do valor original (o `bool()` do Python: 0.0, vazio... são falsos)
    Other(bool),
}

impl Hint {
    /// O `bool()` do Python.
    pub fn truthy(&self) -> bool {
        match self {
            Hint::Bool(b) => *b,
            Hint::Int(n) => *n != 0,
            Hint::Str(s) => !s.is_empty(),
            Hint::Other(truth) => *truth,
        }
    }
}

pub type Hints = HashMap<String, Hint>;

pub fn parse_urgency(value: Option<&Hint>) -> u8 {
    // Como no Python, um booleano conta como número
    let n = match value {
        Some(Hint::Int(n)) => *n,
        Some(Hint::Bool(b)) => *b as i64,
        _ => return NORMAL,
    };
    match n {
        0..=2 => n as u8,
        _ => NORMAL,
    }
}

/// Quanto o toast fica na tela; 0 não expira. Negativo é "o servidor decide".
pub fn toast_ms(urgency: u8, expire_timeout: i32) -> i64 {
    if expire_timeout >= 0 {
        expire_timeout as i64
    } else {
        default_timeout_ms(urgency)
    }
}

pub fn sync_key(hints: &Hints) -> Option<String> {
    SYNC_HINTS.iter().find_map(|name| match hints.get(*name) {
        Some(Hint::Str(s)) if !s.is_empty() => Some(cut(s, MAX_TEXT)),
        _ => None,
    })
}

/// O `urllib.parse.unquote`: os %XX viram bytes, lidos como UTF-8 (o inválido vira �).
fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() && bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit() {
            let hex = |b: u8| (b as char).to_digit(16).unwrap_or(0) as u8;
            out.push(hex(bytes[i + 1]) * 16 + hex(bytes[i + 2]));
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Caminho local de um arquivo: aceita file:// e recusa outros esquemas e relativos.
pub fn local_path(value: &str) -> Option<String> {
    let path = match value.strip_prefix("file://") {
        Some(rest) => {
            // Como o urlsplit: sem tab e quebras, o fragmento e a consulta saem
            let rest: String = rest.chars().filter(|c| !matches!(c, '\t' | '\r' | '\n')).collect();
            let rest = rest.split('#').next().unwrap_or("");
            let rest = rest.split('?').next().unwrap_or("");
            let host_end = rest.find('/').unwrap_or(rest.len());
            if !matches!(&rest[..host_end], "" | "localhost") {
                return None;
            }
            unquote(&rest[host_end..])
        }
        None => value.to_string(),
    };
    path.starts_with('/').then_some(path)
}

/// Só arquivo comum e de tamanho razoável: abrir um FIFO travaria o daemon.
pub fn readable_image(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= MAX_IMAGE_BYTES as u64)
}

/// O que mostrar, na ordem da especificação: caminho absoluto, nome no tema, ou vazio. Texto vazio
/// conta como ausente.
pub fn icon_spec(image_file: &str, image_path: &str, app_icon: &str, entry_icon: &str) -> String {
    if !image_file.is_empty() {
        return image_file.to_string();
    }
    for value in [image_path, app_icon] {
        if value.is_empty() {
            continue;
        }
        match local_path(value) {
            Some(path) if readable_image(Path::new(&path)) => return path,
            Some(_) => {}
            None if !value.contains("://") && !value.contains('/') => return cut(value, MAX_TEXT),
            None => {}
        }
    }
    entry_icon.to_string()
}

/// Confere o cabeçalho de image-data antes de ler o buffer.
pub fn valid_image(width: i64, height: i64, rowstride: i64, has_alpha: bool, bits: i64, channels: i64, size: i64) -> bool {
    if !(0 < width && width <= MAX_IMAGE_SIDE && 0 < height && height <= MAX_IMAGE_SIDE) {
        return false;
    }
    if bits != 8 || channels != if has_alpha { 4 } else { 3 } {
        return false;
    }
    // O limite vale para o tamanho declarado: quem lê completa o buffer até rowstride * height
    if rowstride < width * channels || rowstride.saturating_mul(height) > MAX_IMAGE_BYTES || size > MAX_IMAGE_BYTES {
        return false;
    }
    size >= rowstride * (height - 1) + width * channels
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub icon: String,
    /// Pares (chave, rótulo)
    pub actions: Vec<(String, String)>,
    pub urgency: u8,
    pub timeout_ms: i64,
    /// O tempo foi o app que pediu: ao acabar, a notificação fecha
    pub app_timeout: bool,
    pub transient: bool,
    pub resident: bool,
    pub sync_key: Option<String>,
    pub created_at: f64,
    /// Muda a cada substituição; a tela só refaz o que mudou
    pub revision: u64,
}

impl Notification {
    pub fn has_default(&self) -> bool {
        self.has_action("default")
    }

    pub fn buttons(&self) -> Vec<(String, String)> {
        self.actions.iter().filter(|(key, _)| key != "default").cloned().collect()
    }

    pub fn has_action(&self, key: &str) -> bool {
        self.actions.iter().any(|(k, _)| k == key)
    }

    pub fn as_dict(&self) -> Value {
        let buttons: Vec<Value> = self.buttons().into_iter().map(|(k, t)| json!([k, t])).collect();
        json!({
            "id": self.id, "app_name": self.app_name, "summary": self.summary, "body": self.body,
            "icon": self.icon, "buttons": buttons, "has_default": self.has_default(),
            "urgency": self.urgency, "created_at": self.created_at,
        })
    }
}

/// O que chega num Notify, já tirado do D-Bus.
#[derive(Clone, Debug)]
pub struct Incoming {
    pub app_name: String,
    pub replaces_id: u32,
    /// Já resolvido pelo daemon (icon_spec)
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<String>,
    pub hints: Hints,
    pub expire_timeout: i32,
}

impl Default for Incoming {
    fn default() -> Self {
        Incoming {
            app_name: String::new(),
            replaces_id: 0,
            icon: String::new(),
            summary: String::new(),
            body: String::new(),
            actions: Vec::new(),
            hints: Hints::new(),
            expire_timeout: -1,
        }
    }
}

pub type Clock = Box<dyn Fn() -> f64>;

pub fn system_clock() -> Clock {
    Box::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()))
}

/// A lista em memória. Quem chama avisa os programas: aqui só se diz quem saiu.
pub struct Center {
    clock: Clock,
    pub max_items: usize,
    /// Do mais antigo para o mais novo
    items: Vec<Notification>,
    pub next_id: u32,
    revision: u64,
}

impl Center {
    pub fn new(clock: Clock, max_items: usize) -> Self {
        Center { clock, max_items, items: Vec::new(), next_id: 1, revision: 0 }
    }

    fn position(&self, id: u32) -> Option<usize> {
        self.items.iter().position(|i| i.id == id)
    }

    fn wrap(id: u32) -> u32 {
        id % MAX_ID + 1
    }

    pub fn new_id(&mut self) -> u32 {
        let used: std::collections::HashSet<u32> = self.items.iter().map(|i| i.id).collect();
        while used.contains(&self.next_id) {
            self.next_id = Self::wrap(self.next_id);
        }
        let chosen = self.next_id;
        self.next_id = Self::wrap(chosen);
        chosen
    }

    pub fn find_target(&self, replaces_id: u32, key: Option<&str>, app_name: &str) -> Option<u32> {
        if self.position(replaces_id).is_some() {
            return Some(replaces_id);
        }
        let key = key?;
        self.items.iter().find(|i| i.sync_key.as_deref() == Some(key) && i.app_name == app_name).map(|i| i.id)
    }

    /// Devolve a notificação e as que saíram para respeitar o teto da lista.
    pub fn add(&mut self, incoming: Incoming) -> (Notification, Vec<Notification>) {
        let hints = &incoming.hints;
        let app_name = plain(&incoming.app_name, MAX_TEXT);
        let key = sync_key(hints);
        let target = self.find_target(incoming.replaces_id, key.as_deref(), &app_name);
        let urgency = parse_urgency(hints.get("urgency"));
        self.revision += 1;
        let id = match target {
            Some(id) => id,
            None => self.new_id(),
        };
        let item = Notification {
            id,
            app_name,
            summary: plain(&incoming.summary, MAX_TEXT),
            body: clean_body(&incoming.body),
            icon: incoming.icon,
            actions: parse_actions(&incoming.actions),
            urgency,
            timeout_ms: toast_ms(urgency, incoming.expire_timeout),
            app_timeout: incoming.expire_timeout > 0,
            transient: hints.get("transient").is_some_and(Hint::truthy),
            resident: hints.get("resident").is_some_and(Hint::truthy),
            sync_key: key,
            created_at: (self.clock)(),
            revision: self.revision,
        };
        // Substituir também a leva para o fim, como a mais nova
        if let Some(at) = self.position(id) {
            self.items.remove(at);
        }
        self.items.push(item.clone());
        let evicted = self.trim(id);
        (item, evicted)
    }

    /// Sai a mais antiga; as críticas por último.
    pub fn trim(&mut self, keep: u32) -> Vec<Notification> {
        let mut evicted = Vec::new();
        while self.items.len() > self.max_items {
            let candidates: Vec<usize> = (0..self.items.len()).filter(|at| self.items[*at].id != keep).collect();
            let Some(&first) = candidates.first() else { break };
            let oldest = candidates.iter().copied().find(|at| self.items[*at].urgency != CRITICAL).unwrap_or(first);
            evicted.push(self.items.remove(oldest));
        }
        evicted
    }

    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Notification> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// Todas, as transitórias também, da mais antiga para a mais nova.
    pub fn items(&self) -> &[Notification] {
        &self.items
    }

    pub fn remove(&mut self, id: u32) -> Option<Notification> {
        self.position(id).map(|at| self.items.remove(at))
    }

    pub fn remove_app(&mut self, app_name: &str) -> Vec<Notification> {
        self.take_visible(|i| i.app_name == app_name)
    }

    pub fn clear(&mut self) -> Vec<Notification> {
        self.take_visible(|_| true)
    }

    fn take_visible(&mut self, pick: impl Fn(&Notification) -> bool) -> Vec<Notification> {
        let (removed, kept) = std::mem::take(&mut self.items).into_iter().partition(|i| !i.transient && pick(i));
        self.items = kept;
        removed
    }

    /// O que aparece na lista: as transitórias só existem enquanto dura o toast.
    pub fn visible(&self) -> Vec<&Notification> {
        self.items.iter().filter(|i| !i.transient).collect()
    }
}

/// Não perturbe: desligado, por um tempo ou até desligar.
///
/// O vencimento compara com o relógio: um temporizador não contaria o tempo de suspensão.
pub struct Dnd {
    clock: Clock,
    pub choice: String,
    pub until: Option<f64>,
}

impl Dnd {
    pub fn new(clock: Clock) -> Self {
        Dnd { clock, choice: "off".into(), until: None }
    }

    fn duration(choice: &str) -> Option<f64> {
        match choice {
            "1h" => Some(3600.0),
            "2h" => Some(7200.0),
            _ => None,
        }
    }

    /// "off", "on", "1h", "2h" ou "toggle"; outra opção não muda nada.
    pub fn set(&mut self, choice: &str) -> Result<(), String> {
        let choice = if choice == "toggle" {
            if self.active() { "off" } else { "on" }
        } else {
            choice
        };
        if let Some(seconds) = Self::duration(choice) {
            self.until = Some((self.clock)() + seconds);
        } else if choice == "off" || choice == "on" {
            self.until = None;
        } else {
            return Err(format!("opção desconhecida: {choice}"));
        }
        self.choice = choice.to_string();
        Ok(())
    }

    pub fn active(&mut self) -> bool {
        if self.choice != "off" && self.until.is_some_and(|until| (self.clock)() >= until) {
            self.choice = "off".into();
            self.until = None;
        }
        self.choice != "off"
    }

    pub fn as_dict(&mut self) -> Value {
        let active = self.active();
        json!({"active": active, "choice": self.choice, "until": self.until})
    }
}

/// Uma crítica aparece sempre: com o não perturbe e com a lista aberta (decisão do usuário, 13/11/2026). As
/// outras, nem com a lista aberta, que já as mostra, nem com o não perturbe.
pub fn should_toast(item: &Notification, dnd_active: bool, popup_is_open: bool) -> bool {
    item.urgency == CRITICAL || !(dnd_active || popup_is_open)
}

/// Quem está na tela e quem espera vaga: a ordem é a de chegada.
pub struct ToastQueue {
    pub limit: usize,
    ids: Vec<u32>,
}

impl ToastQueue {
    pub fn new(limit: usize) -> Self {
        ToastQueue { limit, ids: Vec::new() }
    }

    pub fn show(&mut self, id: u32) {
        if !self.ids.contains(&id) {
            self.ids.push(id);
        }
    }

    /// Uma crítica não espera vaga: com a tela cheia, toma o lugar da mais antiga na tela, que só sai da
    /// tela (continua na lista). Devolve o id dela. Uma que esperava vaga e virou crítica (uma
    /// substituição) sobe também.
    pub fn show_urgent(&mut self, id: u32) -> Option<u32> {
        if self.visible().contains(&id) {
            return None; // já na tela: a substituição não muda o lugar
        }
        self.hide(id); // esperava vaga
        if self.ids.len() < self.limit || self.limit == 0 {
            self.show(id);
            return None;
        }
        let oldest = self.ids.remove(0);
        self.ids.insert(self.limit - 1, id);
        Some(oldest)
    }

    pub fn hide(&mut self, id: u32) {
        self.ids.retain(|i| *i != id);
    }

    pub fn clear(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.ids)
    }

    pub fn visible(&self) -> &[u32] {
        &self.ids[..self.ids.len().min(self.limit)]
    }

    /// Todas, as que esperam vaga também.
    pub fn ids(&self) -> &[u32] {
        &self.ids
    }
}

/// O que List devolve: da mais nova para a mais antiga.
pub fn snapshot(center: &Center, dnd: &mut Dnd) -> Value {
    let items: Vec<Value> = center.visible().iter().rev().map(|i| i.as_dict()).collect();
    json!({"dnd": dnd.as_dict(), "items": items})
}

/// Pares (app, itens) com o grupo da notificação mais recente primeiro; items vem nessa ordem.
pub fn group_by_app(items: &[Value]) -> Vec<(String, Vec<Value>)> {
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for item in items {
        let app = item["app_name"].as_str().unwrap_or("").to_string();
        match groups.iter_mut().find(|(name, _)| *name == app) {
            Some((_, list)) => list.push(item.clone()),
            None => groups.push((app, vec![item.clone()])),
        }
    }
    groups
}

/// Há quanto tempo chegou, para mostrar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Age {
    Now,
    Minutes(u64),
    Hours(u64),
    Days(u64),
}

pub fn age(created_at: f64, now: f64) -> Age {
    let seconds = (now - created_at).trunc().max(0.0) as u64;
    match seconds {
        0..=59 => Age::Now,
        60..=3599 => Age::Minutes(seconds / 60),
        3600..=86399 => Age::Hours(seconds / 3600),
        _ => Age::Days(seconds / 86400),
    }
}

/// O texto de `age`, traduzido.
pub fn relative_time(created_at: f64, now: f64) -> String {
    use swaydesk_common::i18n::{ntr, ptr};
    match age(created_at, now) {
        Age::Now => ptr("notification time", "now"),
        Age::Minutes(n) => ptr("notification time", "%d min ago").replace("%d", &n.to_string()),
        Age::Hours(n) => ptr("notification time", "%d h ago").replace("%d", &n.to_string()),
        Age::Days(n) => ntr("%d day ago", "%d days ago", n),
    }
}

/// Ícone da barra: os nomes são os de format-icons na config da Waybar.
pub fn icon_state(snap: &Value) -> &'static str {
    let has_items = snap["items"].as_array().is_some_and(|a| !a.is_empty());
    match (snap["dnd"]["active"].as_bool().unwrap_or(false), has_items) {
        (true, true) => "dnd-notification",
        (true, false) => "dnd-none",
        (false, true) => "notification",
        (false, false) => "none",
    }
}

#[cfg(test)]
mod toast_queue_tests {
    use super::ToastQueue;

    #[test]
    fn a_critical_takes_the_place_of_the_oldest_on_screen() {
        let mut queue = ToastQueue::new(2);
        assert_eq!(queue.show_urgent(1), None); // com vaga, como o show
        queue.show(2);
        queue.show(3); // espera vaga
        assert_eq!(queue.show_urgent(4), Some(1));
        assert_eq!(queue.visible(), [2, 4]);
        assert_eq!(queue.ids(), [2, 4, 3]); // a que esperava continua esperando
        assert_eq!(queue.show_urgent(4), None); // substituição não muda o lugar
        assert_eq!(queue.ids(), [2, 4, 3]);
        // A que esperava e virou crítica sobe
        assert_eq!(queue.show_urgent(3), Some(2));
        assert_eq!(queue.ids(), [4, 3]);
        let mut none = ToastQueue::new(0);
        assert_eq!(none.show_urgent(1), None);
    }
}
