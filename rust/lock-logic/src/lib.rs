//! A lógica do bloqueio de tela, sem GTK: hora e data, nome e foto do usuário, a chave do fundo
//! desfocado, o estado do sistema no canto (bateria, rede, layout do teclado), a capa da mídia e o PAM
//! ([`pam`]). O porte do `sway_lock.py` e do `sway_pam.py`, com os mesmos testes.
//!
//! O fundo desfocado tem a mesma chave e o mesmo arquivo do Python: o cache é um só, e quem gera de
//! antemão é o `sway-lock-blur` (Python), chamado na troca de papel de parede.
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use swaydesk_media::art;

pub mod pam;

pub const WALLPAPER: &str = ".local/share/wallpaper/current.jpg";
pub const ACCOUNTS: &str = "/var/lib/AccountsService";
const CACHE_NAME: &str = "sway-desktop";
/// O desfoque: reduz a 1/BLUR_FACTOR, amplia de novo, e escurece com preto a DARKEN (0 a 255)
pub const BLUR_FACTOR: i32 = 24;
pub const DARKEN: u32 = 110;

/// Os campos do getpwuid que o bloqueio usa.
#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub name: String,
    pub gecos: String,
    pub dir: PathBuf,
}

/// Do getpwuid, nunca do $USER, que quem abriu o programa escolhe.
pub fn current_user() -> Option<User> {
    let mut buffer = vec![0u8; 16 * 1024];
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    let rc = unsafe { libc::getpwuid_r(libc::getuid(), &mut entry, buffer.as_mut_ptr().cast(), buffer.len(), &mut found) };
    if rc != 0 || found.is_null() {
        return None;
    }
    let text = |p: *const libc::c_char| if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() };
    let dir = if entry.pw_dir.is_null() { PathBuf::from("/") } else { PathBuf::from(std::ffi::OsStr::from_bytes(unsafe { CStr::from_ptr(entry.pw_dir) }.to_bytes())) };
    Some(User { name: text(entry.pw_name), gecos: text(entry.pw_gecos), dir })
}

/// A hora: 09:05.
pub fn time_text(now: &libc::tm) -> String {
    format!("{:02}:{:02}", now.tm_hour, now.tm_min)
}

/// O formato da data por extenso segue a língua em que o strftime escreve: o "de" só existe em
/// português. time_locale: o locale efetivo, como "pt_BR".
pub fn date_format(time_locale: &str) -> &'static str {
    if time_locale.starts_with("pt") {
        "%A, %-d de %B"
    } else {
        "%A, %B %-d"
    }
}

/// O strftime da libc, no LC_TIME do processo (quem chama faz o setlocale).
pub fn strftime(format: &str, now: &libc::tm) -> String {
    let Ok(format) = CString::new(format) else { return String::new() };
    let mut buffer = [0u8; 256];
    let n = unsafe { libc::strftime(buffer.as_mut_ptr().cast(), buffer.len(), format.as_ptr(), now) };
    String::from_utf8_lossy(&buffer[..n]).into_owned()
}

/// A data por extenso, com a primeira letra maiúscula.
pub fn date_text(now: &libc::tm, time_locale: &str) -> String {
    let text = strftime(date_format(time_locale), now);
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => text,
    }
}

/// A hora local de agora.
pub fn local_now() -> libc::tm {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm
    }
}

/// O RealName do AccountsService, o primeiro campo do GECOS ou o login, nessa ordem.
pub fn real_name(user: &User, accounts: &Path) -> String {
    let text = std::fs::read(accounts.join("users").join(&user.name)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == "RealName" && !value.trim().is_empty() {
                return value.trim().to_string();
            }
        }
    }
    let first = user.gecos.split(',').next().unwrap_or("");
    let name = if first.is_empty() { user.name.as_str() } else { first }.trim();
    if name.is_empty() { user.name.clone() } else { name.to_string() }
}

/// O ~/.face ou o ícone do AccountsService, se for um arquivo legível e não vazio; senão None (as
/// iniciais).
pub fn photo(user: &User, accounts: &Path) -> Option<PathBuf> {
    [user.dir.join(".face"), accounts.join("icons").join(&user.name)].into_iter().find(|path| {
        let readable = CString::new(path.as_os_str().as_bytes()).is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::R_OK) } == 0);
        readable && std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
    })
}

/// Até duas letras: a primeira do primeiro nome e a do último. "" se não houver letras.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().filter(|w| w.chars().next().is_some_and(char::is_alphanumeric)).collect();
    let (Some(first), Some(last)) = (words.first(), words.last()) else { return String::new() };
    let mut letters: String = first.chars().take(1).collect();
    if words.len() > 1 {
        letters.extend(last.chars().take(1));
    }
    letters.to_uppercase()
}

/// A pasta do cache: $XDG_CACHE_HOME/sway-desktop ou ~/.cache/sway-desktop.
pub fn cache_dir(env: &HashMap<String, String>) -> PathBuf {
    let base = match env.get("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => env.get("HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/")).join(".cache"),
    };
    base.join(CACHE_NAME)
}

/// A chave do fundo desfocado: o arquivo de verdade (o current.jpg é um link para a imagem do dia), o
/// tamanho, a data de modificação e o tamanho do monitor. A mesma do Python (o cache é um só). None se o
/// papel de parede não existir. Sem ler o arquivo inteiro.
pub fn blur_key(wallpaper: &Path, width: i32, height: i32) -> Option<String> {
    let real = std::fs::canonicalize(wallpaper).ok()?;
    let meta = std::fs::metadata(&real).ok()?;
    let mtime_ns = meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128;
    let mut raw = real.as_os_str().as_bytes().to_vec();
    raw.extend_from_slice(format!("\0{}\0{}\0{}x{}", meta.size(), mtime_ns, width, height).as_bytes());
    let digest = Sha256::digest(&raw);
    Some(digest.iter().map(|b| format!("{b:02x}")).collect::<String>()[..32].to_string())
}

pub fn blur_path(wallpaper: &Path, width: i32, height: i32, env: &HashMap<String, String>) -> Option<PathBuf> {
    blur_key(wallpaper, width, height).map(|key| cache_dir(env).join(format!("lock-{key}.png")))
}

/// O fundo desfocado já pronto, ou None. Nunca gera.
pub fn cached_blur(wallpaper: &Path, width: i32, height: i32, env: &HashMap<String, String>) -> Option<PathBuf> {
    blur_path(wallpaper, width, height, env).filter(|p| p.is_file())
}

/// O tamanho para a imagem cobrir o monitor sem faixas, mantendo a proporção. O round do Python
/// arredonda o meio para o par.
pub fn cover_size(src_w: i32, src_h: i32, width: i32, height: i32) -> (i32, i32) {
    let scale = (width as f64 / src_w as f64).max(height as f64 / src_h as f64);
    (width.max((src_w as f64 * scale).round_ties_even() as i32), height.max((src_h as f64 * scale).round_ties_even() as i32))
}

/// Fica com os `keep` fundos mais novos (e sempre o atual).
pub fn prune(folder: &Path, keep: usize, current: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with("lock-") && n.ends_with(".png")))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, old) in files.into_iter().skip(keep) {
        if Some(old.as_path()) != current {
            let _ = std::fs::remove_file(old);
        }
    }
}

/// Os tamanhos lógicos (os do GdkMonitor no bloqueio) dos monitores ativos, sem repetir, do
/// `swaymsg -r -t get_outputs` já lido.
pub fn output_sizes(outputs: &serde_json::Value) -> Vec<(i32, i32)> {
    let mut sizes = Vec::new();
    for output in outputs.as_array().into_iter().flatten() {
        let rect = output.get("rect");
        let side = |key: &str| rect.and_then(|r| r.get(key)).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let size = (side("width"), side("height"));
        if output.get("active").and_then(|v| v.as_bool()) == Some(true) && size.0 > 0 && size.1 > 0 && !sizes.contains(&size) {
            sizes.push(size);
        }
    }
    sizes
}

// --- O estado do sistema no canto: bateria, rede e layout do teclado ---

pub const POWER_SUPPLY: &str = "/sys/class/power_supply";
pub const SYS_NET: &str = "/sys/class/net";
pub const PROC_WIRELESS: &str = "/proc/net/wireless";
/// Os mesmos ícones da barra (waybar-battery e waybar-network)
pub const BATTERY_ICONS: [&str; 5] = ["\u{f244}", "\u{f243}", "\u{f242}", "\u{f241}", "\u{f240}"];
pub const BATTERY_CHARGING: &str = "\u{f0e7}";
pub const BATTERY_PLUGGED: &str = "\u{f1e6}";
pub const WIFI_ICONS: [&str; 4] = ["\u{f091f}", "\u{f0922}", "\u{f0925}", "\u{f0928}"];
pub const ETHERNET_ICON: &str = "\u{f0200}";
pub const OFFLINE_ICON: &str = "\u{f0318}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    Charging,
    Plugged,
    Discharging,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Net {
    Wifi(u32),
    Ethernet,
    Offline,
}

fn read(path: &Path) -> String {
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).trim().to_string()).unwrap_or_default()
}

fn sorted_dir(root: &Path) -> Option<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(root).ok()?.flatten().map(|e| e.path()).collect();
    paths.sort();
    Some(paths)
}

/// (nível, estado) da primeira bateria, ou None sem bateria (o ícone some). Lê o /sys direto.
pub fn battery(root: &Path) -> Option<(u32, Power)> {
    let supplies = sorted_dir(root)?;
    let ac = supplies.iter().any(|d| read(&d.join("type")) == "Mains" && read(&d.join("online")) == "1");
    for supply in &supplies {
        // O mouse e o fone sem fio também aparecem aqui, com scope Device
        if read(&supply.join("type")) != "Battery" || read(&supply.join("scope")) == "Device" {
            continue;
        }
        let level = read(&supply.join("capacity"));
        // O isdigit do Python: só algarismos (o parse aceitaria "+5")
        if level.is_empty() || !level.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(level) = level.parse::<u32>() else { continue };
        let status = read(&supply.join("status"));
        let state = if status == "Charging" {
            Power::Charging
        } else if ac || status == "Full" || status == "Not charging" {
            Power::Plugged
        } else {
            Power::Discharging
        };
        return Some((level.min(100), state));
    }
    None
}

pub fn battery_icon(level: u32, state: Power) -> &'static str {
    match state {
        Power::Charging => BATTERY_CHARGING,
        Power::Plugged => BATTERY_PLUGGED,
        Power::Discharging => BATTERY_ICONS[if level >= 100 { 4 } else { (level / 20) as usize }],
    }
}

/// A qualidade do link em %, do /proc/net/wireless (a escala é 0 a 70), ou None.
pub fn wifi_strength(name: &str, wireless: &Path) -> Option<u32> {
    let text = read(wireless);
    for line in text.lines().skip(2) {
        let Some((iface, rest)) = line.split_once(':') else { continue };
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if iface.trim() == name && fields.len() >= 2 {
            let value: f64 = fields[1].trim_end_matches('.').parse().ok()?;
            return Some((value * 100.0 / 70.0).round_ties_even().clamp(0.0, 100.0) as u32);
        }
    }
    None
}

/// Só placas de verdade (com device) e ligadas; o Wi-Fi ganha do cabo se os dois estiverem no ar.
pub fn network(sys_net: &Path, wireless: &Path) -> Net {
    let Some(ifaces) = sorted_dir(sys_net) else { return Net::Offline };
    let mut wired = false;
    for iface in ifaces {
        let name = iface.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "lo" || !iface.join("device").exists() || read(&iface.join("operstate")) != "up" {
            continue;
        }
        if iface.join("wireless").exists() || iface.join("phy80211").exists() {
            return Net::Wifi(wifi_strength(name, wireless).unwrap_or(0));
        }
        wired = true;
    }
    if wired { Net::Ethernet } else { Net::Offline }
}

pub fn network_icon(net: Net) -> &'static str {
    match net {
        Net::Wifi(s) => WIFI_ICONS[if s < 25 { 0 } else if s < 50 { 1 } else if s < 75 { 2 } else { 3 }],
        Net::Ethernet => ETHERNET_ICON,
        Net::Offline => OFFLINE_ICON,
    }
}

/// O nome do layout ativo do primeiro teclado, do `swaymsg -r -t get_inputs` já lido, e se há mais de
/// um layout para trocar. ("", false) sem teclado.
pub fn keyboard_layout(inputs: &serde_json::Value) -> (String, bool) {
    for device in inputs.as_array().into_iter().flatten() {
        let name = device.get("xkb_active_layout_name").and_then(|v| v.as_str()).unwrap_or("");
        if device.get("type").and_then(|v| v.as_str()) == Some("keyboard") && !name.is_empty() {
            let many = device.get("xkb_layout_names").and_then(|v| v.as_array()).is_some_and(|a| a.len() > 1);
            return (name.to_string(), many);
        }
    }
    (String::new(), false)
}

// --- A mídia: só capas locais na tela bloqueada ---

/// O arquivo da capa, só se já estiver no disco: um file:// ou a do cache do popup de mídia. Nunca
/// baixa nada. None para data: (tratado à parte) e para o resto.
pub fn local_art(url: &str, cache: &Path) -> Option<PathBuf> {
    let path = art::art_path(url, cache)?;
    std::fs::metadata(&path).ok().filter(|m| m.is_file() && m.len() <= art::ART_MAX_BYTES as u64)?;
    Some(path)
}

/// (bytes, formato) da capa, ou None. Só capa local ou data:, e só PNG, JPEG, GIF ou WebP pelos
/// primeiros bytes: os outros carregadores do GdkPixbuf têm histórico de falhas, e o decode roda no
/// processo do bloqueio.
pub fn art_bytes(url: &str, cache: &Path) -> Option<(Vec<u8>, &'static str)> {
    if url.is_empty() {
        return None;
    }
    let data = if url.starts_with("data:") { art::data_url_bytes(url) } else { art::read_limited(&local_art(url, cache)?) }?;
    let kind = art::image_type(&data)?;
    Some((data, kind))
}

#[cfg(test)]
mod tests;
