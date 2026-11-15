//! Os players de mídia pelo MPRIS, sem GTK: o porte de `home/.local/bin/waybar_media.py` (o que o popup
//! lê e os comandos que manda) e as capas.
//!
//! As chamadas são síncronas, com prazo curto: o popup as faz fora da linha principal. O barramento é um
//! parâmetro, para os testes usarem um só deles.
pub mod art;

use gio::glib::{self, Variant, VariantTy};

pub const MPRIS: &str = "org.mpris.MediaPlayer2";
pub const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
pub const PATH: &str = "/org/mpris/MediaPlayer2";
pub const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
const TIMEOUT_MS: i32 = 1000;
pub const PLAYING: &str = "Playing";
pub const PAUSED: &str = "Paused";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// O nome no barramento: org.mpris.MediaPlayer2.firefox.instance_1_42
    pub name: String,
    pub identity: String,
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    /// Microssegundos; 0: desconhecida
    pub length: i64,
    pub track_id: String,
    pub position: i64,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    pub can_pause: bool,
    pub can_play: bool,
    /// O nome único no barramento, de quem vêm os sinais
    pub owner: String,
}

impl State {
    pub fn new(name: &str, status: &str) -> State {
        State { name: name.into(), status: status.into(), ..Default::default() }
    }

    /// Há mídia: tocando ou pausada. Parado ou sem nada, o componente some.
    pub fn active(&self) -> bool {
        self.status == PLAYING || self.status == PAUSED
    }

    /// O nome para a escolha do player.
    pub fn label(&self) -> String {
        if !self.identity.is_empty() {
            return self.identity.clone();
        }
        let rest = self.name.strip_prefix(&format!("{MPRIS}.")).unwrap_or(&self.name);
        rest.split('.').next().unwrap_or("").to_string()
    }
}

/// Um campo de texto do player, que pode mandar qualquer tipo: o que não é texto vira vazio (o caminho
/// de objeto e a assinatura contam como texto, como no `unpack` do Python).
fn text(v: Option<&Variant>) -> String {
    v.and_then(|v| v.str()).map(String::from).unwrap_or_default()
}

/// Um número não negativo: inteiros de qualquer largura, um real (truncado) ou um texto com um inteiro.
/// Um booleano não conta (no Python, bool é um int, e o código o recusa de propósito).
fn number(v: Option<&Variant>) -> i64 {
    let Some(v) = v else { return 0 };
    let n: Option<i64> = if let Some(x) = v.get::<i64>() {
        Some(x)
    } else if let Some(x) = v.get::<u64>() {
        Some(x.min(i64::MAX as u64) as i64)
    } else if let Some(x) = v.get::<i32>() {
        Some(x.into())
    } else if let Some(x) = v.get::<u32>() {
        Some(x.into())
    } else if let Some(x) = v.get::<i16>() {
        Some(x.into())
    } else if let Some(x) = v.get::<u16>() {
        Some(x.into())
    } else if let Some(x) = v.get::<u8>() {
        Some(x.into())
    } else if let Some(x) = v.get::<f64>() {
        x.is_finite().then(|| x.trunc().clamp(i64::MIN as f64, i64::MAX as f64) as i64)
    } else {
        v.str().and_then(|s| s.trim().parse::<i64>().ok())
    };
    n.unwrap_or(0).max(0)
}

fn is_true(v: Option<&Variant>) -> bool {
    v.and_then(|v| v.get::<bool>()) == Some(true)
}

/// Um a{sv} em pares (chave, valor desembrulhado).
fn dict(v: &Variant) -> Vec<(String, Variant)> {
    if !v.type_().is_subtype_of(VariantTy::VARDICT) {
        return Vec::new();
    }
    (0..v.n_children())
        .filter_map(|i| {
            let e = v.child_value(i);
            Some((e.child_value(0).str()?.to_string(), e.child_value(1).as_variant()?))
        })
        .collect()
}

fn lookup<'a>(pairs: &'a [(String, Variant)], key: &str) -> Option<&'a Variant> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// O estado a partir das propriedades da interface Player (um a{sv}). Cada campo é conferido aqui: um
/// player com tipos errados não pode quebrar a janela depois.
pub fn from_props(name: &str, identity: &str, props: &[(String, Variant)]) -> State {
    let meta = lookup(props, "Metadata").map(dict).unwrap_or_default();
    let artist = match lookup(&meta, "xesam:artist") {
        Some(v) if v.str().is_some() => v.str().unwrap_or("").to_string(),
        Some(v) if v.type_().is_array() || v.type_().is_tuple() => {
            let items: Vec<String> = (0..v.n_children())
                .filter_map(|i| {
                    let item = v.child_value(i);
                    // Num av, o item vem embrulhado (o as_variant num não-variante dá um aviso crítico do GLib)
                    let item = if item.type_() == VariantTy::VARIANT { item.as_variant().unwrap_or(item) } else { item };
                    item.str().filter(|s| !s.is_empty()).map(String::from)
                })
                .collect();
            items.join(", ")
        }
        _ => String::new(),
    };
    let status = text(lookup(props, "PlaybackStatus"));
    State {
        name: name.into(),
        identity: identity.into(),
        status: if status.is_empty() { "Stopped".into() } else { status },
        title: text(lookup(&meta, "xesam:title")),
        artist,
        album: text(lookup(&meta, "xesam:album")),
        art_url: text(lookup(&meta, "mpris:artUrl")),
        length: number(lookup(&meta, "mpris:length")),
        track_id: text(lookup(&meta, "mpris:trackid")),
        position: number(lookup(props, "Position")),
        can_next: is_true(lookup(props, "CanGoNext")),
        can_previous: is_true(lookup(props, "CanGoPrevious")),
        can_seek: is_true(lookup(props, "CanSeek")),
        can_pause: is_true(lookup(props, "CanPause")),
        can_play: is_true(lookup(props, "CanPlay")),
        owner: String::new(),
    }
}

/// O player a mostrar: o escolhido, se ainda tiver mídia; senão o primeiro tocando e, sem nenhum, o
/// primeiro pausado. A mesma regra do ícone da barra (waybar-media).
pub fn pick<'a>(states: &'a [State], preferred: Option<&str>) -> Option<&'a State> {
    let active: Vec<&State> = states.iter().filter(|s| s.active()).collect();
    if let Some(chosen) = active.iter().find(|s| Some(s.name.as_str()) == preferred) {
        return Some(chosen);
    }
    active.iter().find(|s| s.status == PLAYING).or(active.first()).copied()
}

/// Microssegundos → 1:05 ou 1:02:03.
pub fn fmt_time(us: i64) -> String {
    let seconds = us.max(0) / 1_000_000;
    let (hours, rest) = (seconds / 3600, seconds % 3600);
    let (minutes, seconds) = (rest / 60, rest % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// O nome de cada botão. Dois do mesmo player (dois mpv) levam também o título da faixa e, sem título,
/// a posição ("mpv (2)"), para se distinguirem.
pub fn player_labels(states: &[State]) -> Vec<String> {
    let labels: Vec<String> = states.iter().map(State::label).collect();
    let mut seen = std::collections::HashMap::new();
    states
        .iter()
        .zip(&labels)
        .map(|(state, label)| {
            if labels.iter().filter(|l| *l == label).count() == 1 {
                return label.clone();
            }
            let n = seen.entry(label.clone()).or_insert(0);
            *n += 1;
            if state.title.is_empty() {
                format!("{label} ({n})")
            } else {
                format!("{label}: {}", state.title)
            }
        })
        .collect()
}

// --- barramento ---

fn call(bus: &gio::DBusConnection, name: &str, iface: &str, method: &str, params: Option<&Variant>) -> Result<Variant, glib::Error> {
    bus.call_sync(Some(name), PATH, iface, method, params, None, gio::DBusCallFlags::NONE, TIMEOUT_MS, gio::Cancellable::NONE)
}

fn dbus_call(bus: &gio::DBusConnection, method: &str, params: Option<&Variant>) -> Result<Variant, glib::Error> {
    bus.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", method, params, None, gio::DBusCallFlags::NONE, TIMEOUT_MS, gio::Cancellable::NONE)
}

pub fn player_names(bus: &gio::DBusConnection) -> Result<Vec<String>, glib::Error> {
    let names = dbus_call(bus, "ListNames", None)?.child_value(0);
    let mut found: Vec<String> = (0..names.n_children()).filter_map(|i| names.child_value(i).str().map(String::from)).filter(|n| n.starts_with(&format!("{MPRIS}."))).collect();
    found.sort();
    Ok(found)
}

/// O nome único (":1.42") de quem tem o nome: os sinais chegam com ele.
pub fn name_owner(bus: &gio::DBusConnection, name: &str) -> Result<String, glib::Error> {
    Ok(dbus_call(bus, "GetNameOwner", Some(&(name,).into()))?.child_value(0).str().unwrap_or("").to_string())
}

/// O estado de um player; None se ele não respondeu. `identity` e `owner`: os já conhecidos.
pub fn read_state(bus: &gio::DBusConnection, name: &str, identity: Option<&str>, owner: Option<&str>) -> Option<State> {
    let props = call(bus, name, "org.freedesktop.DBus.Properties", "GetAll", Some(&(PLAYER,).into())).ok()?.child_value(0);
    let identity = match identity.filter(|i| !i.is_empty()) {
        Some(i) => i.to_string(),
        None => call(bus, name, "org.freedesktop.DBus.Properties", "Get", Some(&(MPRIS, "Identity").into()))
            .ok()
            .and_then(|v| v.child_value(0).as_variant())
            .and_then(|v| v.str().map(String::from))
            .unwrap_or_default(),
    };
    let mut state = from_props(name, &identity, &dict(&props));
    // Só serve para achar o player pelos avisos: sem ele, a releitura é geral
    state.owner = match owner.filter(|o| !o.is_empty()) {
        Some(o) => o.to_string(),
        None => name_owner(bus, name).unwrap_or_default(),
    };
    Some(state)
}

/// O Identity e o dono já conhecidos de um nome.
pub type Known<'a> = &'a dyn Fn(&str) -> (Option<String>, Option<String>);

/// Todos os players, na ordem dos nomes (a mesma do ícone da barra); os que não respondem ficam de fora.
pub fn read_all(bus: &gio::DBusConnection, known: Known) -> Vec<State> {
    let Ok(names) = player_names(bus) else { return Vec::new() };
    names
        .iter()
        .filter_map(|n| {
            let (identity, owner) = known(n);
            read_state(bus, n, identity.as_deref(), owner.as_deref())
        })
        .collect()
}

pub fn position(bus: &gio::DBusConnection, name: &str) -> Option<i64> {
    let v = call(bus, name, "org.freedesktop.DBus.Properties", "Get", Some(&(PLAYER, "Position").into())).ok()?;
    v.child_value(0).as_variant()?.get::<i64>()
}

/// PlayPause, Next ou Previous. Devolve se o player aceitou.
pub fn command(bus: &gio::DBusConnection, name: &str, method: &str) -> bool {
    call(bus, name, PLAYER, method, None).is_ok()
}

/// Vai para `target` (µs). Com o id da faixa, SetPosition; sem ele (alguns players), Seek relativo.
pub fn seek_to(bus: &gio::DBusConnection, state: &State, target: i64) -> bool {
    let target = target.max(0).min(if state.length > 0 { state.length } else { target.max(0) });
    if state.track_id != NO_TRACK && Variant::is_object_path(&state.track_id) {
        let Ok(track) = glib::variant::ObjectPath::try_from(state.track_id.clone()) else { return false };
        return call(bus, &state.name, PLAYER, "SetPosition", Some(&(track, target).into())).is_ok();
    }
    let Some(current) = position(bus, &state.name) else { return false };
    call(bus, &state.name, PLAYER, "Seek", Some(&(target - current,).into())).is_ok()
}

#[cfg(test)]
mod tests;
