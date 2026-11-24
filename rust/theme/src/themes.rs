//! Os temas GTK instalados e a troca de tema em todo o desktop: o porte do `sway_themes.py`.
use std::collections::BTreeMap;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use swaydesk_common::config::{self, contrast_with_white, is_accent, load_config, papirus_color, Config, Palette, DEFAULT_ACCENT};
use swaydesk_common::i18n::tr;
use swaydesk_common::resident::own_uid;

use crate::desktop::{self as dc, ICONS_WRAPPER};
use crate::families::{self as sf, Style};
use crate::papirus as sp;
use crate::{command_name, fill, fill_one, run, run_timeout, Error};

/// Em ordem: a primeira definição encontrada é a cor do tema
const COLOR_NAMES: &[&str] = &["accent_color", "theme_selected_bg_color", "selected_bg_color"];
/// Com o branco: acima disso, a cor está mais perto do preto que do branco
const MID_GREY_CONTRAST: f64 = 4.58;
const FALLBACK_GSETTINGS: &str = "/usr/bin/gsettings";
const DCONF: &str = "/usr/bin/dconf";
const INTERFACE: &str = "org.gnome.desktop.interface";
/// Os ícones de bandeja do KDE só existem no Breeze: com o Breeze Dark instalado, o tema gerado o herda
const FALLBACK_ICONS: &str = "breeze-dark";

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    /// None: o tema não define a cor no CSS
    pub accent: Option<String>,
    /// None: não há ícones com o nome correspondente
    pub icons: Option<String>,
    pub dark: bool,
}

pub fn accent_of(css: &str) -> Option<String> {
    COLOR_NAMES.iter().find_map(|name| dc::defined_color(css, name))
}

fn read_lossy(path: &Path) -> std::io::Result<String> {
    std::fs::read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Se o tema de ícones, ou um dos que ele herda, tem ícones coloridos de apps.
pub fn has_app_icons(name: &str, seen: &mut Vec<String>) -> bool {
    if seen.iter().any(|s| s == name) || name == "hicolor" {
        return false;
    }
    seen.push(name.to_string());
    for base in dc::icon_dirs() {
        let index = base.join(name).join("index.theme");
        if !index.is_file() {
            continue;
        }
        let Ok(text) = read_lossy(&index) else { return false };
        let line = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=').map(str::to_string));
        for folder in line("Directories").map(|d| d.split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>()).unwrap_or_default() {
            let parts: Vec<&str> = folder.split('/').collect();
            if parts.contains(&"apps") && !parts.iter().any(|p| p.contains("symbolic")) && base.join(name).join(&folder).is_dir() {
                return true;
            }
        }
        let inherits = line("Inherits").map(|i| i.split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>()).unwrap_or_default();
        return inherits.iter().any(|parent| has_app_icons(parent, seen));
    }
    false
}

/// O nome sem os -Darker, -Dark e -dark seguidos de - ou do fim (o `-(Darker|Dark|dark)(?=-|$)` do Python).
fn without_dark(theme: &str) -> String {
    let mut out = String::new();
    let mut rest = theme;
    'scan: while !rest.is_empty() {
        if rest.starts_with('-') {
            for token in ["-Darker", "-Dark", "-dark"] {
                if let Some(after) = rest.strip_prefix(token) {
                    if after.is_empty() || after.starts_with('-') {
                        rest = after;
                        continue 'scan;
                    }
                }
            }
        }
        let c = rest.chars().next().unwrap_or_default();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Mint-Y-Dark-Red usa os ícones Mint-Y-Red. Sem ícones de apps no tema correspondente, ficam os atuais.
pub fn icons_for(theme: &str) -> Option<String> {
    let name = without_dark(theme);
    (dc::icons_installed(&name) && has_app_icons(&name, &mut Vec::new())).then_some(name)
}

/// Pelas cores do tema, porque o nome engana. Sem cores no CSS, o nome.
pub fn dark_of(name: &str, css: &str) -> bool {
    let background = dc::defined_color(css, "theme_bg_color");
    let text = dc::defined_color(css, "theme_fg_color");
    match (background, text) {
        (Some(bg), Some(fg)) => contrast_with_white(&fg) < contrast_with_white(&bg),
        (Some(bg), None) => contrast_with_white(&bg) > MID_GREY_CONTRAST,
        _ => dc::is_dark(name),
    }
}

pub fn theme_is_dark(name: &str) -> bool {
    for base in dc::theme_dirs() {
        if let Ok(css) = read_lossy(&base.join(name).join("gtk-3.0/gtk.css")) {
            return dark_of(name, &css);
        }
    }
    dc::is_dark(name)
}

fn read_theme(name: &str, css_file: &Path) -> std::io::Result<Theme> {
    let css = read_lossy(css_file)?;
    Ok(Theme { name: name.to_string(), accent: accent_of(&css), icons: icons_for(name), dark: dark_of(name, &css) })
}

/// O casefold do Python, para a ordem: o minúsculo basta para os nomes de temas.
fn fold(name: &str) -> String {
    name.to_lowercase()
}

/// Os temas com GTK 3, por nome. O tema do usuário vence o do sistema com o mesmo nome.
pub fn discover() -> Vec<Theme> {
    let mut found: BTreeMap<String, Theme> = BTreeMap::new();
    for base in dc::theme_dirs() {
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let css = entry.path().join("gtk-3.0/gtk.css");
            if !found.contains_key(&name) && css.is_file() {
                if let Ok(theme) = read_theme(&name, &css) {
                    found.insert(name, theme);
                }
            }
        }
    }
    let mut themes: Vec<Theme> = found.into_values().collect();
    themes.sort_by_key(|t| fold(&t.name)); // estável: no empate do casefold, a ordem do nome
    themes
}

/// O aviso para quem pediu um tema que não existe. None: está instalado.
pub fn missing_note(config: &Config) -> Option<String> {
    if dc::theme_installed(&config.gtk_theme) {
        return None;
    }
    Some(fill(&tr("%(wanted)s is not installed: using %(used)s"), &[("wanted", &config.gtk_theme), ("used", &dc::effective_gtk_theme(config))]))
}

/// Por que a cor de destaque não foi para os gtk.css: outro desktop instalado (o GTK os lê em toda sessão), ou um
/// gtk.css que não é daqui. Os mesmos textos do gtk_css_notes do sway_themes.py.
pub fn gtk_css_notes(kept: &[String], others: &[String]) -> Vec<String> {
    let mut notes = Vec::new();
    if !others.is_empty() {
        notes.push(fill_one(
            &tr("Another desktop is installed (%s), so the accent color is not written to ~/.config/gtk-3.0/gtk.css and gtk-4.0/gtk.css, which every desktop reads: in Sway, the libadwaita apps use the nearest of their own accent colors, and the GTK 3 apps keep the theme's color"),
            &others.join(", "),
        ));
    }
    let css: Vec<String> = kept.iter().filter(|n| n.ends_with("gtk.css")).map(|n| format!("~/.config/{n}")).collect();
    if !css.is_empty() {
        notes.push(fill_one(&tr("%s is not from the Theme app and was left as it was: the GTK apps don't get the accent color from it"), &css.join(", ")));
    }
    notes
}

/// As notas do gtk.css como as coisas estão agora, sem aplicar nada: o aviso fixo do app Tema (antes, só aparecia
/// depois de Aplicar, e sumia).
pub fn gtk_css_status() -> Vec<String> {
    gtk_css_notes(&dc::gtk_css_kept(&dc::config_home()), &dc::other_desktops())
}

/// O aviso para o adw-gtk3 com o snapd-desktop-integration (o Ubuntu Desktop): os snaps não o têm, e o snapd pede a
/// cada login um snap de terceiros do tema (desktop::snap_theme_prompt). None: sem o problema.
pub fn snap_note(theme: &str) -> Option<String> {
    snap_note_with(theme, dc::snap_theme_prompt())
}

pub fn snap_note_with(theme: &str, snaps: bool) -> Option<String> {
    (snaps && theme.starts_with("adw-gtk3")).then(|| tr("Snap apps don't include adw-gtk3: snapd will ask at every login to install a third-party theme snap. Choose Adwaita to avoid this; snaps already have it."))
}

static KDE_IMPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"@import\s+(url\()?['"]?colors\.css"#).unwrap());

/// O aviso para um Breeze com o esquema de cores do KDE. None: sem o problema.
pub fn kde_colors_note(theme: &str) -> Option<String> {
    if !theme.starts_with("Breeze") {
        return None;
    }
    let base = dc::config_home().join("gtk-3.0");
    let css = read_lossy(&base.join("gtk.css")).ok()?;
    if !KDE_IMPORT.is_match(&css) || !base.join("colors.css").is_file() {
        return None;
    }
    Some(fill_one(
        &tr("%s takes its colors from the KDE color scheme (~/.config/gtk-3.0/colors.css): the GTK 3 apps follow it, not the style chosen here. Change the scheme in the KDE System Settings"),
        theme,
    ))
}

/// As sessões em que o app é da casa: o Sway e os outros compositores com layer-shell do projeto.
pub const OWN_DESKTOPS: [&str; 4] = ["sway", "hyprland", "river", "niri"];

/// O aviso para quem abre o app dentro de outro desktop (GNOME, KDE, Cinnamon...): o tema GTK e o de ícones
/// que ele grava são os mesmos desse desktop. Pelo XDG_CURRENT_DESKTOP; o SWAYSOCK confirma o Sway. Sem
/// XDG_CURRENT_DESKTOP, nenhum outro desktop. O `foreign_desktop_note` do sway_themes.py.
pub fn foreign_desktop_note(current_desktop: &str, swaysock: &str) -> Option<String> {
    let names: Vec<&str> = current_desktop.split(':').filter(|n| !n.is_empty()).collect();
    if names.is_empty() || !swaysock.is_empty() || names.iter().any(|n| OWN_DESKTOPS.contains(&n.to_lowercase().as_str())) {
        return None;
    }
    Some(fill_one(
        &tr("Open outside Sway (%s): the GTK and icon themes applied here also change the settings of that desktop"),
        &names.join(":"),
    ))
}

/// O mesmo, com o ambiente do processo.
pub fn foreign_desktop_note_here() -> Option<String> {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    foreign_desktop_note(&var("XDG_CURRENT_DESKTOP"), &var("SWAYSOCK"))
}

/// Roda a mudança na trava do sway-theme: dois cliques, ou o login e o app ao mesmo tempo, não gravam o
/// config e o tema de ícones juntos.
pub fn locked<T>(change: impl FnOnce() -> T) -> Result<T, Error> {
    let run_dir = std::env::var("XDG_RUNTIME_DIR").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/tmp".into());
    // Sem truncar e sem seguir link: fora da sessão, a trava cai no /tmp, onde outro usuário poderia deixar
    // um link para um arquivo nosso
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(format!("{run_dir}/sway-theme.lock"))?;
    // SAFETY: o flock recebe um descritor aberto, que vive até o fim da função
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(Error::Io(std::io::Error::last_os_error()));
    }
    Ok(change()) // a trava sai ao fechar o arquivo
}

/// A pasta de estado do que veio da sway-desktop (accent-auto, icon-theme, as cópias de antes do tema): a nova
/// ou a velha, pelo `swaydesk_common::dirs`. Os marcadores que sempre foram da swaydesk/ não passam por aqui.
pub fn state_dir() -> PathBuf {
    swaydesk_common::dirs::state_dir()
}

/// Guarda o arquivo do usuário antes da primeira alteração. Depois, ele já é nosso.
fn backup_once(file: &Path, label: &str) -> std::io::Result<()> {
    let copy = state_dir().join("antes-do-sway-theme").join(label);
    if file.exists() && !copy.exists() {
        if let Some(parent) = copy.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(file, &copy)?;
        if let Ok(meta) = std::fs::metadata(file) {
            if let Ok(modified) = meta.modified() {
                let _ = std::fs::File::options().write(true).open(&copy).and_then(|f| f.set_modified(modified));
            }
        }
    }
    Ok(())
}

fn check_toml(text: &str) -> Result<(), Error> {
    text.parse::<toml::Table>().map(|_| ()).map_err(|err| Error::Value(err.message().to_string()))
}

/// O config.toml em duas partes: as chaves do começo e, a partir da primeira tabela ([effects]) com os
/// comentários logo acima dela, o resto. O split_tables do sway_themes.py.
pub fn split_tables(text: &str) -> (String, &str) {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1).filter(|&i| i < text.len()));
    let line = |i: usize| text[starts[i]..].lines().next().unwrap_or("");
    let Some(mut cut) = (0..starts.len()).find(|&i| line(i).trim_start_matches([' ', '\t']).starts_with('[')) else {
        return (text.to_string(), "");
    };
    while cut > 0 && line(cut - 1).trim_start().starts_with('#') {
        cut -= 1;
    }
    let at = starts.get(cut).copied().unwrap_or(text.len());
    (text[..at].to_string(), &text[at..])
}

/// Um valor do config.toml para o set_config_values.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Bool(bool),
    Int(i64),
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::Str(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::Str(value.to_string())
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Value::Int(i64::from(value))
    }
}

/// Troca só estas linhas do config.toml: os comentários e o resto ficam como estão. As chaves são as do começo
/// do arquivo, antes da primeira tabela ([effects]): uma chave nova entra ali, senão viraria parte da tabela.
pub fn set_config_values(file: &Path, values: &[(&str, Value)]) -> Result<(), Error> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err.into()),
    };
    check_toml(&text)?; // um arquivo inválido é do usuário: não muda nada
    let (mut head, tail) = split_tables(&text);
    let mut appended = false;
    for (key, value) in values {
        let line = match value {
            Value::Str(text) => format!("{key} = {}", serde_json::to_string(text).unwrap_or_default()),
            Value::Bool(flag) => format!("{key} = {flag}"),
            Value::Int(n) => format!("{key} = {n}"),
        };
        let pattern = Regex::new(&format!(r"(?m)^{}\s*=.*$", regex::escape(key))).expect("key pattern");
        if pattern.is_match(&head) {
            head = pattern.replacen(&head, 1, regex::NoExpand(&line)).into_owned();
        } else {
            if !head.is_empty() && !head.ends_with('\n') {
                head.push('\n');
            }
            head.push_str(&line);
            head.push('\n');
            appended = true;
        }
    }
    if appended && !tail.is_empty() && !head.ends_with("\n\n") {
        head.push('\n');
    }
    let text = head + tail;
    check_toml(&text)?;
    dc::write_file(&dc::resolved(file), &text)?;
    Ok(())
}

/// A cor das pastas do Papirus: a da config ou, sem ela, a mais perto do destaque. None se os ícones em uso
/// não são do Papirus.
pub fn folder_color(config: &Config) -> Option<String> {
    let name = dc::installed_icon_theme(config);
    let names = if sp::is_papirus(&name) { sp::available_colors(sp::theme_dir(&name).as_deref()) } else { Vec::new() };
    if names.contains(&config.folder_color) {
        return Some(config.folder_color.clone());
    }
    (!names.is_empty()).then(|| sp::nearest_color(&config.accent, &names))
}

/// O nome a gravar no dconf. Com o Papirus, o tema gerado, com as pastas na cor; com o Breeze Dark
/// instalado, ele herda o escolhido e o Breeze Dark. Com outro tema, o gerado herda o escolhido e, para os ícones
/// que faltam nele (os da Ajuda, da dock, dos popups), o Papirus no estilo (dark), o Adwaita e o Breeze Dark.
pub fn gtk_icon_theme(name: &str, folders: Option<&str>, dark: bool, remove: bool) -> String {
    if ["breeze", FALLBACK_ICONS, ICONS_WRAPPER].contains(&name) {
        return name.to_string();
    }
    let inherits = icon_inherits(name, dark, dc::icons_installed);
    let colored = if sp::is_papirus(name) { folders.filter(|f| !f.is_empty()) } else { None };
    // Sem cor nas pastas e sem nenhum de reserva instalado, o gerado não acrescenta nada
    if colored.is_none() && !inherits.iter().any(|i| i != name && i != "hicolor") {
        if remove {
            sp::remove();
        }
        return name.to_string();
    }
    match sp::build(name, &inherits, colored) {
        Ok(()) => ICONS_WRAPPER.to_string(),
        Err(_) => name.to_string(),
    }
}

/// O Inherits do tema gerado: o escolhido, os de reserva instalados (para o Papirus, só o Breeze Dark) e o hicolor.
pub fn icon_inherits(name: &str, dark: bool, installed: impl Fn(&str) -> bool) -> Vec<String> {
    let fallbacks: &[&str] = if sp::is_papirus(name) { &[FALLBACK_ICONS] } else { &[sf::icons_for_style(if dark { Style::Dark } else { Style::Light }), "Adwaita", FALLBACK_ICONS] };
    let mut inherits = vec![name.to_string()];
    inherits.extend(fallbacks.iter().filter(|f| **f != name && installed(f)).map(|f| f.to_string()));
    if name != "hicolor" {
        inherits.push("hicolor".to_string());
    }
    inherits
}

/// Gera (ou apaga) o tema dos ícones da config e devolve o nome a gravar no dconf.
pub fn config_icon_theme(config: &Config, remove: bool) -> String {
    let dark = theme_is_dark(&dc::effective_gtk_theme(config));
    gtk_icon_theme(&dc::installed_icon_theme(config), folder_color(config).as_deref(), dark, remove)
}

/// Os ícones pedidos (--icons, o seletor do app): um dos que dá para escolher; o Papirus no estilo do tema.
pub fn resolve_icons(name: &str, chosen: &Theme) -> Result<String, Error> {
    let entry = crate::icon_themes::entry_of(name);
    if !crate::icon_themes::installed("").iter().any(|t| t.id == entry) {
        return Err(Error::Value(fill(&tr("unknown icon theme: %(name)s. See them with: %(command)s icons"), &[("name", name), ("command", command_name())])));
    }
    if entry != crate::icon_themes::PAPIRUS {
        return Ok(entry.to_string());
    }
    let style = sf::icons_for_style(sf::style_of(chosen));
    Ok(std::iter::once(style).chain(sp::THEMES.iter().copied()).find(|n| dc::icons_installed(n)).unwrap_or(style).to_string())
}

fn dconf_values(config: &Config, icons: Option<String>) -> Vec<(&'static str, String)> {
    let theme = dc::effective_gtk_theme(config);
    let scheme = if theme_is_dark(&theme) { "prefer-dark" } else { "default" };
    vec![
        ("gtk-theme", theme),
        ("icon-theme", icons.unwrap_or_else(|| config_icon_theme(config, true))),
        ("color-scheme", scheme.to_string()),
    ]
}

/// O pid de um SWAYSOCK (sway-ipc.<uid>.<pid>.sock).
fn swaysock_pid(sock: &str) -> Option<String> {
    static SOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"sway-ipc\.\d+\.(\d+)\.sock$").unwrap());
    SOCK.captures(sock).map(|c| c[1].to_string())
}

/// O DCONF_PROFILE da sessão do Sway deste usuário ("" se ela não o tem), ou None sem sessão à vista. Serve a
/// um ambiente sem o DCONF_PROFILE da sessão (ssh, cron, um shell que exportou só o XDG_CURRENT_DESKTOP).
/// Primeiro o arquivo que o sway-session grava no login (session_file_profile); sem ele (uma sessão aberta
/// antes dele), os processos (sway_profile_in). SWAY_DESKTOP_PROC troca o /proc nos testes.
pub fn running_sway_profile() -> Option<String> {
    // SAFETY: getuid não falha nem toca memória
    let uid = unsafe { libc::getuid() };
    let proc = std::env::var_os("SWAY_DESKTOP_PROC").filter(|v| !v.is_empty()).map_or_else(|| PathBuf::from("/proc"), PathBuf::from);
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty());
    if let Some(profile) = runtime.and_then(|dir| session_file_profile(&Path::new(&dir).join(SESSION_PROFILE_FILE), &proc, uid)) {
        return Some(profile);
    }
    let sock_pid = std::env::var("SWAYSOCK").ok().as_deref().and_then(swaysock_pid);
    sway_profile_in(&proc, sock_pid.as_deref(), uid)
}

/// O arquivo do sway-session, no XDG_RUNTIME_DIR: o pid do Sway e o caminho do perfil (vazio numa sessão sem ele).
const SESSION_PROFILE_FILE: &str = "swaydesk/dconf-profile";

/// O perfil do arquivo do sway-session: só um arquivo comum do uid (um link não vale), e só com o Sway que o
/// gravou vivo (a pasta dura mais que ele: linger, um ssh, os segundos depois do logout; senão, None, e vale a
/// busca nos processos). Um perfil que não existe mais (o pacote saiu) vale como sessão sem perfil. Lê no
/// máximo 4 KiB; o conteúdo só decide se há perfil, nunca vira caminho de gravação.
fn session_file_profile(file: &Path, proc: &Path, uid: u32) -> Option<String> {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(file).ok()?;
    if !meta.is_file() || meta.uid() != uid {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(file).ok()?.take(4096).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines();
    let pid = lines.next()?.trim();
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) || !our_sway_alive(proc, pid, uid) {
        return None;
    }
    let profile = lines.next().unwrap_or_default().trim();
    Some(if !profile.is_empty() && Path::new(profile).is_file() { profile.to_string() } else { String::new() })
}

/// Se o pid é um sway vivo do uid. Pelo comm e pelo uid real do status, que se leem mesmo no sway do Arch
/// (cap_sys_nice: não "dumpable", a pasta dele no /proc aparece como do root).
fn our_sway_alive(proc: &Path, pid: &str, uid: u32) -> bool {
    let dir = proc.join(pid);
    let is_sway = std::fs::read_to_string(dir.join("comm")).is_ok_and(|c| c.trim() == "sway");
    is_sway
        && std::fs::read_to_string(dir.join("status")).is_ok_and(|status| {
            status
                .lines()
                .find_map(|l| l.strip_prefix("Uid:"))
                .and_then(|ids| ids.split_whitespace().next()?.parse::<u32>().ok())
                == Some(uid)
        })
}

/// A sessão a seguir, entre os processos `sway` e `waybar` do uid (os de outro usuário não contam, como no
/// pgrep -u): o Sway do SWAYSOCK, se for um deles (um socket velho não é); senão, do menor pid, o primeiro com o
/// DCONF_PROFILE (um Sway aninhado de teste, sem o perfil, não decide). A Waybar herda o ambiente da sessão e,
/// ao contrário do Sway do Arch (cap_sys_nice), tem o environ legível. Um environ que não se lê não conta. Do
/// environ só se lê o DCONF_PROFILE, e nada dele vai para o log. "" quando há sessão nossa mas nenhuma com o
/// perfil.
fn sway_profile_in(proc: &Path, sock_pid: Option<&str>, uid: u32) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    // None: não é um processo da sessão que se possa ler; Some(perfil, ou "" sem ele)
    let profile_of = |pid: &str, names: &[&str]| -> Option<String> {
        let dir = proc.join(pid);
        let ours = std::fs::read_to_string(dir.join("comm")).is_ok_and(|c| names.contains(&c.trim()))
            && std::fs::metadata(&dir).is_ok_and(|m| m.uid() == uid);
        let environ = ours.then(|| std::fs::read(dir.join("environ")).ok()).flatten()?;
        let value = environ.split(|b| *b == 0).find_map(|item| item.strip_prefix(b"DCONF_PROFILE="));
        Some(value.map(|v| String::from_utf8_lossy(v).into_owned()).unwrap_or_default())
    };
    if let Some(profile) = sock_pid.and_then(|pid| profile_of(pid, &["sway"])) {
        return Some(profile);
    }
    let mut scanned: Vec<u64> = std::fs::read_dir(proc)
        .map(|dir| dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect())
        .unwrap_or_default();
    scanned.sort_unstable();
    let mut seen = false;
    for pid in scanned {
        match profile_of(&pid.to_string(), &["sway", "waybar"]) {
            Some(profile) if !profile.is_empty() => return Some(profile),
            Some(_) => seen = true,
            None => {}
        }
    }
    seen.then(String::new)
}

/// Um Sway iniciado por um sway-session anterior ao perfil próprio lê a base normal do dconf. Sem o
/// DCONF_PROFILE no ambiente, vale o do Sway em execução: num shell de fora da sessão, ele pode não chegar.
pub fn legacy_session() -> bool {
    if std::env::var_os("DCONF_PROFILE").is_some_and(|v| !v.is_empty())
        || !std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|v| v == "sway")
    {
        return false;
    }
    running_sway_profile().is_none_or(|p| p.is_empty())
}

pub fn profiles() -> PathBuf {
    dc::share_dir().join("dconf")
}

/// Se a sessão do Sway lê o banco próprio. Quem o liga é o sway-session, que exporta o DCONF_PROFILE: nos
/// dotfiles, quando ~/.local/share/sway-desktop/dconf/sway (ou a da swaydesk) existe; nos pacotes (--package), o
/// do /usr/share/sway-desktop. Fora da sessão (ssh, cron), sem a variável, vale o Sway em execução: com os pacotes
/// não há arquivo no ~, e o tema ia para a base normal, que a sessão não lê (visto na VM: os apps GTK 4 ficavam
/// com o tema antigo). Um Sway sem o perfil (aberto do tty, sem o sway-session) lê a base normal.
fn sway_profile_in_use() -> bool {
    std::env::var_os("DCONF_PROFILE").is_some_and(|v| !v.is_empty())
        || swaydesk_common::dirs::data_dir().join("dconf/sway").is_file()
        || running_sway_profile().is_some_and(|p| !p.is_empty())
}

/// O DCONF_PROFILE da base do Sway, de onde quer que se rode. Numa sessão antiga, ou sem o banco próprio em
/// uso (nenhum Sway com o perfil, nem o dos dotfiles), nenhum.
fn dconf_profile(profile: &str) -> Option<PathBuf> {
    (!legacy_session() && sway_profile_in_use()).then(|| profiles().join(profile))
}

fn written_icons_file() -> PathBuf {
    state_dir().join("icon-theme")
}

/// O último tema de ícones que este código gravou no dconf; "" se não há registro.
fn written_icons() -> String {
    std::fs::read_to_string(written_icons_file()).map(|t| t.trim().to_string()).unwrap_or_default()
}

fn write_dconf(values: &[(&str, String)]) -> bool {
    let gsettings = std::env::var("SWAY_THEME_GSETTINGS").unwrap_or_else(|_| FALLBACK_GSETTINGS.into());
    let profile = dconf_profile("sway");
    let mut all = true;
    let mut icons_ok = false;
    for (key, value) in values {
        let ok = run(&[&gsettings, "set", INTERFACE, key, value], profile.as_deref()) == 0;
        all &= ok;
        if *key == "icon-theme" {
            icons_ok = ok;
        }
    }
    if icons_ok {
        if let Some((_, icons)) = values.iter().find(|(k, _)| *k == "icon-theme") {
            let _ = dc::write_file(&written_icons_file(), &format!("{icons}\n"));
        }
    }
    all
}

/// Em Wayland, o GTK lê o tema daqui, pelo portal. O tamanho do cursor (para os apps que leem o gsettings: GTK 3,
/// Electron, o XWayland pelo portal) vai junto só na base própria do Sway: na base de todos (uma sessão antiga), a
/// troca de tema mudaria o cursor do GNOME; ali, só o `sway-theme cursor` o grava.
pub fn set_dconf(config: &Config, icons: Option<String>) -> bool {
    let mut values = dconf_values(config, icons);
    if dconf_profile("sway").is_some() {
        values.push(("cursor-size", config.cursor_size.to_string()));
    }
    write_dconf(&values)
}

/// O valor só na base do Sway, como o dconf o mostra ('Mint-Y-Teal'); "" sem valor; None se falhou.
fn dconf_read(key: &str) -> Option<String> {
    let dconf = std::env::var("SWAY_THEME_DCONF").unwrap_or_else(|_| DCONF.into());
    let mut command = std::process::Command::new(dconf);
    command.args(["read", &format!("/{}/{key}", INTERFACE.replace('.', "/"))]);
    if let Some(profile) = dconf_profile("sway-only") {
        command.env("DCONF_PROFILE", profile);
    }
    let (code, out) = run_timeout(&mut command, Duration::from_secs(10))?;
    (code == 0).then(|| out.trim().to_string())
}

/// Só se a leitura funcionou e veio vazia: na dúvida, o valor é de alguém.
fn dconf_is_unset(key: &str) -> bool {
    dconf_read(key).as_deref() == Some("")
}

/// No login: sem um tema na base do Sway, grava o da config. Com um tema gravado, só o ícone.
pub fn init_dconf(config: &Config) -> bool {
    // O tamanho do cursor: na base própria do Sway, sempre o do config (um cursor_size mudado à mão vale no login);
    // na base de todos, só se ela não tem nenhum, como o resto. Sem conseguir ler, nada
    let size = config.cursor_size.to_string();
    let own = dconf_profile("sway").is_some();
    let size_ok = match dconf_read("cursor-size") {
        Some(current) if current != size && (own || current.is_empty()) => write_dconf(&[("cursor-size", size)]),
        _ => true,
    };
    if !dconf_is_unset("gtk-theme") {
        return refresh_icons(config) && size_ok;
    }
    let mut values = dconf_values(config, None);
    // O mesmo do seat do Sway (00-cursor.conf): sem isso, o cursor mudaria ao passar por cima dos apps GTK
    if let Some(cursor) = swaydesk_common::cursor::cursor_theme() {
        values.push(("cursor-theme", cursor.to_string()));
    }
    let unset: Vec<(&str, String)> = values.into_iter().filter(|(key, _)| dconf_is_unset(key)).collect();
    write_dconf(&unset) && size_ok
}

/// Com um tema já gravado, só o ícone, e só se ainda é o da config. Outro valor é do usuário.
pub fn refresh_icons(config: &Config) -> bool {
    let wanted = config_icon_theme(config, true);
    let current = dconf_read("icon-theme");
    let written = written_icons();
    let names = [
        config.icon_theme.clone(),
        dc::installed_icon_theme(config),
        ICONS_WRAPPER.to_string(),
        if written.is_empty() { ICONS_WRAPPER.to_string() } else { written },
    ];
    let ours = |value: &str| names.iter().any(|n| value == format!("'{n}'"));
    match current {
        Some(current) if ours(&current) && current != format!("'{wanted}'") => write_dconf(&[("icon-theme", wanted)]),
        _ => true,
    }
}

static KITTY_INCLUDE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*include\s+current-theme\.conf\s*$").unwrap());

/// None se o kitty não usa este arquivo.
fn update_kitty(colors: &Palette) -> Result<Option<bool>, std::io::Error> {
    let base = dc::config_home().join("kitty");
    let Ok(config) = read_lossy(&base.join("kitty.conf")) else { return Ok(None) };
    if !KITTY_INCLUDE.is_match(&config) {
        return Ok(None);
    }
    let theme = base.join("current-theme.conf");
    backup_once(&theme, "kitty-current-theme.conf")?;
    dc::write_file(&dc::resolved(&theme), &dc::render_template("kitty-theme.conf", colors)?)?;
    run(&["pkill", "-USR1", "-u", &own_uid(), "-x", "kitty"], None);
    Ok(Some(true))
}

static HERDR_SECTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\[theme\.custom\][ \t]*$").unwrap());
static HERDR_ACCENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^accent\s*=.*$").unwrap());

/// Só a linha accent de [theme.custom]. None: não há o que trocar.
fn update_herdr(colors: &Palette) -> Result<Option<bool>, std::io::Error> {
    let file = dc::config_home().join("herdr/config.toml");
    let Ok(text) = std::fs::read_to_string(&file) else { return Ok(None) };
    let Some(header) = HERDR_SECTION.find(&text) else { return Ok(None) };
    let start = header.end();
    // Até a próxima linha que abre uma seção, ou o fim
    let end = text[start..].find("\n[").map_or(text.len(), |i| start + i + 1);
    let body = &text[start..end];
    if !HERDR_ACCENT.is_match(body) {
        return Ok(None);
    }
    let line = format!("accent = \"{}\"", colors.accent);
    let body = HERDR_ACCENT.replacen(body, 1, regex::NoExpand(&line));
    dc::write_file(&dc::resolved(&file), &format!("{}{}{}", &text[..start], body, &text[end..]))?;
    Ok(Some(run(&["herdr", "server", "reload-config"], None) == 0))
}

/// Encerra o programa e espera ele sair. False: continuou rodando.
fn stop(name: &str) -> bool {
    run(&["pkill", "-u", &own_uid(), "-x", name], None);
    for _ in 0..30 {
        if run(&["pgrep", "-u", &own_uid(), "-x", name], None) != 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Espera o programa voltar sozinho, pelo laço do config do Sway (que espera 1 s antes de reabrir).
fn came_back(name: &str) -> bool {
    for _ in 0..30 {
        if run(&["pgrep", "-u", &own_uid(), "-x", name], None) == 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Espera a bandeja da Waybar nova.
fn wait_for_tray() {
    for _ in 0..50 {
        let code = run(&["busctl", "--user", "status", "org.kde.StatusNotifierWatcher"], None);
        if code == 0 || code == 127 {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// O programa que o laço do config do Sway reabre: o de ~/.local/bin nos dotfiles, ou, na sessão do pacote
/// swaydesk-desktop, o da pasta dos scripts dele (/usr/libexec/swaydesk). Só com os componentes, sem uma
/// das duas sessões, não há esse laço: encerrar o programa o deixaria fechado até o próximo login.
fn in_session_loop(name: &str) -> Option<PathBuf> {
    if swaydesk_common::paths::package_session() {
        return swaydesk_common::paths::find_helper(name);
    }
    let path = config::home().join(".local/bin").join(name);
    path.is_file().then_some(path)
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

/// Só o que lê as cores ao abrir. O sway-notifd fica: a lista de notificações só existe nele.
fn restart_desktop() -> Vec<String> {
    let mut failures = Vec::new();
    // Nos dotfiles, o mesmo texto da versão em Python (o sh do Sway expande o ~); na sessão do pacote, os
    // caminhos dos scripts dele
    let command = match in_session_loop("sway-waybar") {
        Some(waybar) if swaydesk_common::paths::package_session() => {
            in_session_loop("sway-after-env").map(|after_env| format!("{} {}", shell_quote(&after_env), shell_quote(&waybar)))
        }
        Some(_) => Some("~/.local/bin/sway-after-env ~/.local/bin/sway-waybar".to_string()),
        None => None,
    };
    if let Some(command) = command.filter(|_| run(&["pgrep", "-u", &own_uid(), "-x", "waybar"], None) == 0) {
        stop("waybar");
        if run(&["swaymsg", "exec", &command], None) != 0 {
            failures.push("Waybar".to_string());
        } else {
            wait_for_tray();
        }
    }
    if run(&["pgrep", "-u", &own_uid(), "-x", "sway-drawer"], None) == 0 {
        // O lançador dos dotfiles ou, instalada por pacote, o swaydesk-drawer
        let drawer = swaydesk_common::paths::component("sway-drawer", "swaydesk-drawer");
        let start = format!("{} start", shell_quote(&drawer));
        // O laço do config do Sway a reabre (desde a 0.1.4). Uma sessão aberta antes, sem o laço, ou só com os
        // componentes, não: aí o start vem daqui. Os dois juntos não abrem duas, pela trava
        if !stop("sway-drawer") || !(came_back("sway-drawer") || run(&["swaymsg", "exec", &start], None) == 0) {
            failures.push(tr("drawer"));
        }
    }
    for name in ["sway-dock", "sway-osd-bar"] {
        if in_session_loop(name).is_some() {
            run(&["pkill", "-u", &own_uid(), "-x", name], None); // o laço do config do Sway o abre de novo
        }
    }
    if run(&["pgrep", "-u", &own_uid(), "-x", "wob"], None) == 0 {
        failures.push(tr("volume indicator (log in again)"));
    }
    failures
}

/// Os ícones que o apply grava para o tema. Trocar o tema ou a cor (o accent-auto também) nunca troca os ícones
/// que a pessoa escolheu: um tema de ícones instalado que não é o Papirus fica, a não ser que seja o que veio com
/// o tema GTK em uso (Mint-Y-Red com o Mint-Y-Dark-Red), que acompanha o tema novo. Com o Papirus em uso (ou o do
/// config não instalado), o Papirus no estilo do tema; sem ele, os ícones com o nome do tema ou os instalados.
pub fn chosen_icons(chosen: &Theme, current: &Config) -> String {
    let papirus = sf::icons_for_style(sf::style_of(chosen));
    let current_theme_icons = icons_for(&current.gtk_theme);
    pick_icons(&current.icon_theme, current_theme_icons.as_deref(), papirus, chosen.icons.as_deref(), dc::icons_installed, || {
        dc::installed_icon_theme(current)
    })
}

/// A regra do chosen_icons, sem o disco. current_theme_icons: os ícones com o nome do tema GTK em uso; installed
/// diz se um tema de ícones existe; fallback, o instalado da lista de reserva.
pub fn pick_icons(
    current: &str,
    current_theme_icons: Option<&str>,
    papirus: &str,
    theme_icons: Option<&str>,
    installed: impl Fn(&str) -> bool,
    fallback: impl FnOnce() -> String,
) -> String {
    if !sp::is_papirus(current) && installed(current) && current_theme_icons != Some(current) {
        return current.to_string();
    }
    if installed(papirus) {
        return papirus.to_string();
    }
    theme_icons.map_or_else(fallback, str::to_string)
}

fn check_folders(folders: Option<&str>) -> Result<(), Error> {
    match folders {
        Some(color) if !color.is_empty() && papirus_color(color).is_none() => {
            Err(Error::Value(fill(&tr("unknown folder color: %(color)s. See them with: %(command)s colors"), &[("color", color), ("command", command_name())])))
        }
        _ => Ok(()),
    }
}

pub struct Applied {
    pub notes: Vec<String>,
    pub failures: Vec<String>,
}

/// Aplica a variante da família no estilo, com a cor do Papirus. Devolve o tema e o resultado. auto: o do apply.
pub fn pick(family: &str, style: Style, folders: &str, auto: Option<bool>, icons: Option<&str>) -> Result<(String, Applied), Error> {
    // O "" (a pasta mais perto do destaque) só vale no set: o pick tira a cor do nome
    if folders.is_empty() {
        return Err(Error::Value(fill(&tr("unknown folder color: %(color)s. See them with: %(command)s colors"), &[("color", folders), ("command", command_name())])));
    }
    check_folders(Some(folders))?;
    let themes = discover();
    let (families, _others) = sf::group(&themes);
    let Some(found) = families.iter().find(|f| fold(&f.name) == fold(family)) else {
        return Err(Error::Value(fill(&tr("theme family not installed: %(family)s. See them with: %(command)s families"), &[("family", family), ("command", command_name())])));
    };
    let accent = papirus_color(folders).unwrap_or_default();
    let Some(theme) = sf::resolve(found, style, accent, folders) else {
        return Err(Error::Value(fill(&tr("%(family)s has no %(style)s version"), &[("family", &found.name), ("style", &style.label())])));
    };
    let applied = apply(&theme, Some(accent), Some(folders), Some(themes), auto, icons)?;
    Ok((theme, applied))
}

/// O accent_auto que o apply grava, ou None para não mexer na chave. auto: Some(true) (--auto, a cor veio do papel)
/// e Some(false) (--no-auto, o interruptor do app desligado) gravam sempre. None: a mesma cor não mexe na chave (o
/// modo fica como está) e outra cor o desliga, mas só num config que já tem a chave ou com o modo ligado
/// (current.accent_auto, que sem a chave vem do accent padrão): ligado sem a chave, o false fica explícito. A mesma
/// regra do sway_themes.py.
fn accent_auto_value(auto: Option<bool>, color: &str, current: &Config, has_key: bool) -> Option<bool> {
    if auto.is_some() {
        return auto;
    }
    (color != current.accent && (current.accent_auto || has_key)).then_some(false)
}

/// Se o config.toml tem a chave (solta, antes das tabelas). Ilegível: não.
fn has_key(file: &Path, key: &str) -> bool {
    std::fs::read_to_string(file).ok().and_then(|text| text.parse::<toml::Table>().ok()).is_some_and(|t| t.contains_key(key))
}

/// Grava o cursor_size e o aplica: os arquivos gerados (o seat do Sway e os settings.ini), o gsettings da base do
/// Sway e, numa sessão do Sway aberta, o seat na hora e o XCURSOR_* do systemd e do D-Bus. Um tamanho fora do
/// CURSOR_SIZES é erro antes de mudar qualquer coisa. O mesmo do apply_cursor_size do sway_themes.py.
pub fn apply_cursor_size(size: u32) -> Result<Applied, Error> {
    if !config::CURSOR_SIZES.contains(&i64::from(size)) {
        return Err(Error::Value(fill(&tr("invalid cursor size: %(size)s (use %(min)s to %(max)s)"), &[
            ("size", &size.to_string()),
            ("min", &config::CURSOR_SIZES.start().to_string()),
            ("max", &config::CURSOR_SIZES.end().to_string()),
        ])));
    }
    let file = config::path();
    let problems = load_config(&file).1;
    if problems.iter().any(|p| p.contains("could not read")) {
        return Err(Error::Value(fill_one(&tr("fix the file first: %s"), &problems[0])));
    }
    set_config_values(&file, &[("cursor_size", size.into())])?;
    let config = load_config(&file).0;
    dc::generate(&config)?;
    let mut notes = Vec::new();
    let mut failures = Vec::new();
    if !write_dconf(&[("cursor-size", size.to_string())]) {
        failures.push(tr("cursor size in dconf (gsettings)"));
    }
    let size = size.to_string();
    // Sem nenhum dos temas, o "default" do Xcursor, como no 00-cursor.conf
    let theme = swaydesk_common::cursor::cursor_theme().unwrap_or("default");
    // Só com o seat trocado (uma sessão do Sway): fora dela, o XCURSOR_* iria para os apps de outro desktop
    if run(&["swaymsg", "seat", "seat0", "xcursor_theme", theme, &size], None) == 0 {
        let env = [format!("XCURSOR_THEME={theme}"), format!("XCURSOR_SIZE={size}")];
        if run(&["dbus-update-activation-environment", "--systemd", &env[0], &env[1]], None) != 0 {
            failures.push(tr("cursor size in the session environment"));
        }
        notes.push(tr("Older apps (GTK 3, Qt 5, Electron, X11) take the new size when reopened"));
    } else {
        notes.push(tr("No Sway session: the rest changes when you log in to it"));
    }
    Ok(Applied { notes, failures })
}

/// Aplica tudo. Erra antes de mudar qualquer coisa. folders: a cor das pastas do Papirus, só delas ("": a mais perto
/// do destaque). auto: Some(true) grava accent_auto = true e Some(false), false. None: uma cor diferente da atual é
/// escolha manual e grava false; a mesma cor (só o tema ou o estilo mudou) deixa o modo como está. Sem o Some, a
/// chave só é gravada num config que já a tem ou com o modo ligado (o padrão, sem a chave, quando o accent é o
/// DEFAULT_ACCENT).
pub fn apply(theme: &str, accent: Option<&str>, folders: Option<&str>, themes: Option<Vec<Theme>>, auto: Option<bool>, icons: Option<&str>) -> Result<Applied, Error> {
    let available = themes.unwrap_or_else(discover);
    let Some(chosen) = available.iter().find(|t| t.name == theme).cloned() else {
        return Err(Error::Value(fill(&tr("theme not installed: %(theme)s. See them with: %(command)s list"), &[("theme", theme), ("command", command_name())])));
    };
    if let Some(accent) = accent.filter(|a| !is_accent(a)) {
        return Err(Error::Value(fill(&tr("invalid color: %(color)s (use one like %(example)s)"), &[("color", accent), ("example", DEFAULT_ACCENT)])));
    }
    check_folders(folders)?;
    let wanted_icons = icons.map(|name| resolve_icons(name, &chosen)).transpose()?;
    let file = config::path();
    let (current, problems) = load_config(&file);
    if problems.iter().any(|p| p.contains("could not read")) {
        return Err(Error::Value(fill_one(&tr("fix the file first: %s"), &problems[0])));
    }
    // Antes de gravar o config.toml: sem os modelos (um binário sem o share ao lado), o tema ficava gravado
    // e o resto não rodava
    let templates = dc::templates();
    if dc::TEMPLATE_NAMES.iter().any(|name| !templates.join(name).is_file()) {
        return Err(Error::Value(fill_one(&tr("templates not found in %s: run ./install.sh"), &templates.display().to_string())));
    }
    let mut notes = Vec::new();
    let mut failures = Vec::new();
    let color = accent.map(str::to_string).or_else(|| chosen.accent.clone()).unwrap_or_else(|| current.accent.clone()).to_lowercase();
    if accent.is_none() && chosen.accent.is_none() {
        notes.push(fill(&tr("%(theme)s does not define an accent color: kept %(color)s. To change it, use --accent"), &[("theme", theme), ("color", &color)]));
    }
    let icons = match wanted_icons {
        Some(icons) => {
            notes.push(fill_one(&tr("Icons: %s"), &icons));
            icons
        }
        None => {
            let icons = chosen_icons(&chosen, &current);
            if chosen.icons.is_none() && icons != sf::icons_for_style(sf::style_of(&chosen)) {
                notes.push(fill(&tr("There are no icons for %(theme)s: kept %(icons)s"), &[("theme", theme), ("icons", &icons)]));
            }
            icons
        }
    };
    let folder_value = folders.map_or_else(|| current.folder_color.clone(), str::to_string);
    let auto_value = accent_auto_value(auto, &color, &current, has_key(&file, "accent_auto"));
    let mut values: Vec<(&str, Value)> =
        vec![("gtk_theme", theme.into()), ("icon_theme", icons.into()), ("accent", color.into()), ("folder_color", folder_value.into())];
    if let Some(value) = auto_value {
        values.push(("accent_auto", value.into()));
    }
    set_config_values(&file, &values)?;
    let config = load_config(&file).0;
    let colors = config::palette(&config);
    // Antes de gerar os settings.ini e o qt5ct: eles usam o tema gerado, se ele existir
    let icon_name = config_icon_theme(&config, true);
    let kept = dc::generate(&config)?;
    if kept.iter().any(|name| name.ends_with("settings.ini")) {
        notes.push(tr("The GTK settings.ini files belong to another desktop (KDE, for example) and were left as they were: the X11 apps in Sway follow their theme"));
    }
    notes.extend(gtk_css_notes(&kept, &dc::other_desktops()));
    if let Some(note) = kde_colors_note(theme) {
        notes.push(note);
    }
    if let Some(note) = snap_note(theme) {
        notes.push(note);
    }
    if !set_dconf(&config, Some(icon_name)) {
        failures.push(tr("theme in dconf (gsettings)"));
    }
    // Só com os dotfiles: é o install.sh --system que resolve. Só com os pacotes, não há install.sh (e a grade
    // exporta XDG_CURRENT_DESKTOP=sway aos apps no Sway aberto do tty, o que faria a sessão parecer antiga)
    if legacy_session() && sway_profile_in_use() {
        notes.push(tr("This session uses the same dconf database as GNOME and KDE: to separate them, run ./install.sh --system and log in again"));
    }
    for (name, result) in [("kitty", update_kitty(&colors)), ("herdr", update_herdr(&colors))] {
        match result {
            Ok(Some(false)) => failures.push(name.to_string()),
            Ok(_) => {}
            Err(err) => {
                notes.push(format!("{name}: {err}"));
                failures.push(name.to_string());
            }
        }
    }
    if dc::reload_sway() {
        failures.extend(restart_desktop());
    } else {
        notes.push(tr("No Sway session: the rest changes when you log in to it"));
    }
    notes.push(tr("Notifications change at the next login. Some open apps may need to be reopened"));
    Ok(Applied { notes, failures })
}

/// O que o accent-auto fez.
pub enum Auto {
    /// accent_auto = false: nada
    Off,
    /// unreadable: o papel não existe ou não abre (vale o DEFAULT_ACCENT). applied: o tema aplicado e o
    /// resultado; None se a cor já era a de destaque
    Done { unreadable: bool, applied: Option<(String, Applied)> },
}

/// Com accent_auto = true, a cor do Papirus do papel de parede ou, sem cor nele ou sem papel legível, o
/// DEFAULT_ACCENT, aplicada como na escolha manual do app Tema: na família e no estilo em uso, a variante do tema
/// GTK mais perto da cor; fora das famílias, o tema em uso com a cor. Cor igual à de destaque: nada roda, sem
/// piscar a barra.
pub fn accent_auto() -> Result<Auto, Error> {
    let config = load_config(&config::path()).0;
    // Sem a chave e sem o arquivo do papel, o modo está desligado (Config::accent_auto_for): nada a aplicar, e
    // nenhum aviso de papel ilegível a cada login de quem nunca teve papel
    if !config.accent_auto_for(crate::wallpaper_accent::wallpaper().exists()) {
        return Ok(Auto::Off);
    }
    let reading = crate::wallpaper_accent::current_color();
    let name = reading.flatten().unwrap_or_default();
    let hex = papirus_color(name).unwrap_or(DEFAULT_ACCENT);
    let unreadable = reading.is_none();
    if hex == config.accent {
        return Ok(Auto::Done { unreadable, applied: None });
    }
    let themes = discover();
    let theme = dc::effective_gtk_theme(&config);
    let (families, _) = sf::group(&themes);
    let variant = sf::family_of(&theme)
        .and_then(|(f, style)| families.iter().find(|x| x.name == f && x.has_style(style)).and_then(|x| sf::resolve(x, style, hex, name)))
        .unwrap_or(theme);
    let applied = apply(&variant, Some(hex), Some(name), Some(themes), Some(true), None)?;
    Ok(Auto::Done { unreadable, applied: Some((variant, applied)) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um /proc falso com processos sway (pid, perfil: None sem o DCONF_PROFILE), todos do usuário que roda o teste.
    fn fake_proc(name: &str, sways: &[(u32, Option<&str>)]) -> PathBuf {
        let proc = std::env::temp_dir().join(format!("swaydesk-theme-proc-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&proc);
        for (pid, profile) in sways {
            let dir = proc.join(pid.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("comm"), "sway\n").unwrap();
            if *pid < 10 {
                // Os pids < 10: o sway do Arch (cap_sys_nice), cujo environ não se lê
                continue;
            }
            let mut environ = b"HOME=/home/x\0SECRET=nada\0".to_vec();
            if let Some(profile) = profile {
                environ.extend_from_slice(format!("DCONF_PROFILE={profile}\0").as_bytes());
            }
            std::fs::write(dir.join("environ"), environ).unwrap();
        }
        // Outro programa com o perfil não conta
        let other = proc.join("3");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("comm"), "bash\n").unwrap();
        std::fs::write(other.join("environ"), b"DCONF_PROFILE=/outro\0").unwrap();
        proc
    }

    /// Um processo no /proc falso, com o comm e o uid real do status (o que se lê do sway do Arch).
    fn fake_process(proc: &Path, pid: u32, comm: &str, uid: u32) {
        let dir = proc.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("comm"), format!("{comm}\n")).unwrap();
        std::fs::write(dir.join("status"), format!("Name:\t{comm}\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n")).unwrap();
    }

    #[test]
    fn the_session_file_is_only_trusted_when_it_is_ours() {
        // SAFETY: getuid não falha nem toca memória
        let uid = unsafe { libc::getuid() };
        let dir = std::env::temp_dir().join(format!("swaydesk-theme-session-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let proc = dir.join("proc");
        // 50: o sway do Arch, sem environ legível; 60: outro programa; 70: um sway de outro usuário
        fake_process(&proc, 50, "sway", uid);
        fake_process(&proc, 60, "bash", uid);
        fake_process(&proc, 70, "sway", uid.wrapping_add(1));
        let profile = dir.join("sway");
        std::fs::write(&profile, "user-db:sway\nuser-db:user\n").unwrap();
        let file = dir.join("dconf-profile");
        let write = |pid: &str, profile: &str| std::fs::write(&file, format!("{pid}\n{profile}\n")).unwrap();
        let shown = profile.display().to_string();
        write("50", &shown);
        assert_eq!(session_file_profile(&file, &proc, uid), Some(shown.clone()));
        assert_eq!(session_file_profile(&file, &proc, uid.wrapping_add(1)), None);
        assert_eq!(session_file_profile(&dir.join("nada"), &proc, uid), None);
        // O Sway que gravou já saiu (o pid não existe, ou é outro programa, ou de outro usuário): não vale
        for pid in ["99", "60", "70", "", "x"] {
            write(pid, &shown);
            assert_eq!(session_file_profile(&file, &proc, uid), None, "{pid}");
        }
        // Um perfil que não existe mais, ou vazio: sessão sem perfil
        write("50", &dir.join("sumiu").display().to_string());
        assert_eq!(session_file_profile(&file, &proc, uid).as_deref(), Some(""));
        write("50", "");
        assert_eq!(session_file_profile(&file, &proc, uid).as_deref(), Some(""));
        // UTF-8 inválido não derruba a leitura (o mesmo do Python)
        std::fs::write(&file, b"50\n\xff\n").unwrap();
        assert_eq!(session_file_profile(&file, &proc, uid).as_deref(), Some(""));
        // Um link não vale
        let target = dir.join("alvo");
        std::fs::write(&target, format!("50\n{shown}\n")).unwrap();
        std::fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(&target, &file).unwrap();
        assert_eq!(session_file_profile(&file, &proc, uid), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_running_sway_profile_is_picked_deterministically() {
        // SAFETY: getuid não falha nem toca memória
        let uid = unsafe { libc::getuid() };
        // O de menor pid sem o perfil (um Sway aninhado de teste) não decide: o que tem o perfil
        let proc = fake_proc("two", &[(10, None), (20, Some("/usr/share/sway-desktop/dconf/sway"))]);
        assert_eq!(sway_profile_in(&proc, None, uid).as_deref(), Some("/usr/share/sway-desktop/dconf/sway"));
        // O do SWAYSOCK vale, mesmo sem o perfil; um SWAYSOCK velho (pid que não é sway) cai na busca
        assert_eq!(sway_profile_in(&proc, Some("10"), uid).as_deref(), Some(""));
        assert_eq!(sway_profile_in(&proc, Some("3"), uid).as_deref(), Some("/usr/share/sway-desktop/dconf/sway"));
        assert_eq!(sway_profile_in(&proc, Some("99"), uid).as_deref(), Some("/usr/share/sway-desktop/dconf/sway"));
        // Os Sway de outro usuário (o dono dos arquivos não é o uid) não contam
        assert_eq!(sway_profile_in(&proc, None, uid.wrapping_add(1)), None);
        assert_eq!(sway_profile_in(&proc, Some("20"), uid.wrapping_add(1)), None);
        // O Sway do Arch (environ ilegível) não conta, nem pelo SWAYSOCK; a Waybar da sessão sim, mas não pelo
        // SWAYSOCK
        let arch = fake_proc("arch", &[(5, None)]);
        assert_eq!(sway_profile_in(&arch, Some("5"), uid), None);
        let waybar = arch.join("30");
        std::fs::create_dir_all(&waybar).unwrap();
        std::fs::write(waybar.join("comm"), "waybar\n").unwrap();
        std::fs::write(waybar.join("environ"), b"DCONF_PROFILE=/usr/share/sway-desktop/dconf/sway\0").unwrap();
        assert_eq!(sway_profile_in(&arch, Some("5"), uid).as_deref(), Some("/usr/share/sway-desktop/dconf/sway"));
        assert_eq!(sway_profile_in(&arch, Some("30"), uid).as_deref(), Some("/usr/share/sway-desktop/dconf/sway"));
        let _ = std::fs::remove_dir_all(&arch);
        // Só Sway sem o perfil: "", nenhum: None
        let none = fake_proc("bare", &[(10, None)]);
        assert_eq!(sway_profile_in(&none, None, uid).as_deref(), Some(""));
        let empty = fake_proc("empty", &[]);
        assert_eq!(sway_profile_in(&empty, None, uid), None);
        for dir in [proc, none, empty] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }


    #[test]
    fn adw_gtk3_with_snapd_desktop_integration_gets_a_note() {
        // Os mesmos casos do test_snap_note do tests/test_sway_theme.py
        assert!(snap_note_with("adw-gtk3-dark", true).is_some_and(|n| n.contains("Adwaita")));
        assert!(snap_note_with("adw-gtk3", true).is_some());
        assert!(snap_note_with("adw-gtk3-dark", false).is_none());
        assert!(snap_note_with("Adwaita-dark", true).is_none());
        assert!(snap_note_with("", true).is_none());
    }

    // Os mesmos casos do test_gtk_css_notes do tests/test_sway_theme.py
    #[test]
    fn gtk_css_notes_say_why_the_accent_is_missing() {
        assert!(gtk_css_notes(&[], &[]).is_empty());
        assert!(gtk_css_notes(&["gtk-3.0/settings.ini".into()], &[]).is_empty());
        let notes = gtk_css_notes(&[], &["gnome-wayland".into(), "plasma".into()]);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].starts_with("Another desktop is installed (gnome-wayland, plasma), so the accent color"));
        let notes = gtk_css_notes(&["gtk-3.0/gtk.css".into(), "gtk-4.0/gtk.css".into()], &[]);
        assert_eq!(notes, ["~/.config/gtk-3.0/gtk.css, ~/.config/gtk-4.0/gtk.css is not from the Theme app and was left as it was: the GTK apps don't get the accent color from it"]);
    }

    #[test]
    fn config_values_go_before_the_tables() {
        let dir = std::env::temp_dir().join(format!("swaydesk-setcfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        std::fs::write(&file, "locale = \"pt_BR\"\naccent = \"#111111\"\n\n[enabled]\ndock = false\naccent = 1\n").unwrap();
        set_config_values(&file, &[("accent", "#222222".into()), ("gtk_theme", "Novo".into())]).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let table: toml::Table = text.parse().unwrap();
        assert_eq!(table["accent"].as_str(), Some("#222222"));
        assert_eq!(table["gtk_theme"].as_str(), Some("Novo"));
        // A chave de mesmo nome dentro da tabela não é a solta
        assert_eq!(table["enabled"]["accent"].as_integer(), Some(1));
        assert_eq!(table["enabled"]["dock"].as_bool(), Some(false));
    }

    #[test]
    fn bool_values_and_the_accent_auto_key() {
        let dir = std::env::temp_dir().join(format!("swaydesk-setbool-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        std::fs::write(&file, "accent = \"#111111\"\n\n[effects]\nblur = true\n").unwrap();
        assert!(!has_key(&file, "accent_auto"));
        set_config_values(&file, &[("accent_auto", true.into())]).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\naccent_auto = true\n"), "{text}");
        assert!(has_key(&file, "accent_auto"));
        set_config_values(&file, &[("accent_auto", false.into())]).unwrap();
        let (config, warnings) = swaydesk_common::config::parse_config(&std::fs::read_to_string(&file).unwrap());
        assert!(!config.accent_auto && warnings.is_empty(), "{warnings:?}");
        assert!(!has_key(&dir.join("missing.toml"), "accent_auto"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_wrapper_inherits_fallbacks_for_the_missing_icons() {
        let all = |_: &str| true;
        assert_eq!(icon_inherits("Yaru", true, all), ["Yaru", "Papirus-Dark", "Adwaita", "breeze-dark", "hicolor"]);
        assert_eq!(icon_inherits("Yaru", false, all), ["Yaru", "Papirus", "Adwaita", "breeze-dark", "hicolor"]);
        // Sem repetir o escolhido, e só os instalados
        assert_eq!(icon_inherits("Adwaita", true, all), ["Adwaita", "Papirus-Dark", "breeze-dark", "hicolor"]);
        assert_eq!(icon_inherits("Yaru", true, |n| n == "Adwaita"), ["Yaru", "Adwaita", "hicolor"]);
        assert_eq!(icon_inherits("Yaru", true, |_| false), ["Yaru", "hicolor"]);
        assert_eq!(icon_inherits("hicolor", true, |_| false), ["hicolor"]);
        // O Papirus, como antes: só o Breeze Dark de reserva
        assert_eq!(icon_inherits("Papirus-Dark", true, all), ["Papirus-Dark", "breeze-dark", "hicolor"]);
    }

    #[test]
    fn icons_stay_unless_papirus_is_in_use() {
        // (em uso, instalados, ícones com o nome do tema) → o gravado; o tema escolhido é escuro (Papirus-Dark), e
        // o tema em uso não traz ícones com o nome dele
        let cases: &[(&str, &[&str], Option<&str>, &str)] = &[
            // Um tema de ícones da pessoa, instalado, fica: com ou sem o Papirus, com ou sem ícones do tema
            ("Yaru", &["Yaru", "Papirus-Dark", "Mint-Y-Red"], Some("Mint-Y-Red"), "Yaru"),
            ("Yaru", &["Yaru"], None, "Yaru"),
            ("Novo-Red", &["Novo-Red", "Papirus-Dark"], None, "Novo-Red"),
            // O Papirus em uso (qualquer um dos três) segue o estilo do tema
            ("Papirus", &["Papirus", "Papirus-Dark"], Some("Mint-Y-Red"), "Papirus-Dark"),
            ("Papirus-Light", &["Papirus-Light", "Papirus-Dark"], None, "Papirus-Dark"),
            // O do config não instalado: o Papirus, se houver; senão os do tema; senão a reserva
            ("Sumiu", &["Papirus-Dark", "Mint-Y-Red"], Some("Mint-Y-Red"), "Papirus-Dark"),
            ("Papirus-Dark", &["Mint-Y-Red"], Some("Mint-Y-Red"), "Mint-Y-Red"),
            ("Papirus-Dark", &["Adwaita"], None, "reserva"),
        ];
        for (current, installed, theme_icons, expected) in cases {
            let got = pick_icons(current, None, "Papirus-Dark", *theme_icons, |n| installed.contains(&n), || "reserva".into());
            assert_eq!(&got, expected, "{current} {installed:?} {theme_icons:?}");
        }
        // Os ícones que vieram com o tema em uso (Mint-Y-Red do Mint-Y-Dark-Red) acompanham o tema novo:
        // (em uso, ícones do tema em uso, instalados, ícones do tema novo) → o gravado
        let theme_cases: &[(&str, &str, &[&str], Option<&str>, &str)] = &[
            ("Mint-Y-Red", "Mint-Y-Red", &["Mint-Y-Red", "Mint-Y-Blue"], Some("Mint-Y-Blue"), "Mint-Y-Blue"),
            ("Mint-Y-Red", "Mint-Y-Red", &["Mint-Y-Red", "Papirus-Dark"], Some("Mint-Y-Blue"), "Papirus-Dark"),
            ("Mint-Y-Red", "Mint-Y-Red", &["Mint-Y-Red"], None, "reserva"),
            // Escolhidos à mão, de outro nome que o do tema em uso, ficam
            ("Yaru", "Mint-Y-Red", &["Yaru", "Mint-Y-Red", "Papirus-Dark"], Some("Mint-Y-Blue"), "Yaru"),
        ];
        for (current, in_use, installed, theme_icons, expected) in theme_cases {
            let got = pick_icons(current, Some(in_use), "Papirus-Dark", *theme_icons, |n| installed.contains(&n), || "reserva".into());
            assert_eq!(&got, expected, "{current} {in_use} {installed:?} {theme_icons:?}");
        }
    }

    #[test]
    fn accent_auto_value_rules() {
        let derived_on = Config::default(); // sem a chave, com o destaque padrão
        let off = Config { accent: "#e25252".into(), accent_auto: false, ..Config::default() };
        let on = Config { accent: "#e25252".into(), accent_auto: true, ..Config::default() };
        // --auto: sempre true, mesmo num config que nunca teve a chave; --no-auto: sempre false, mesmo com a mesma cor
        assert_eq!(accent_auto_value(Some(true), "#5294e2", &off, false), Some(true));
        assert_eq!(accent_auto_value(Some(false), "#e25252", &on, true), Some(false));
        assert_eq!(accent_auto_value(Some(false), DEFAULT_ACCENT, &derived_on, false), Some(false));
        // Outra cor à mão: false explícito, também com o modo ligado só pela regra de sem a chave
        assert_eq!(accent_auto_value(None, "#5294e2", &derived_on, false), Some(false));
        assert_eq!(accent_auto_value(None, "#5294e2", &on, true), Some(false));
        // A mesma cor (só o tema ou o estilo): a chave fica como está, presente ou não
        assert_eq!(accent_auto_value(None, "#e25252", &on, true), None);
        assert_eq!(accent_auto_value(None, DEFAULT_ACCENT, &derived_on, false), None);
        assert_eq!(accent_auto_value(None, "#e25252", &off, true), None);
        // Desligado e sem a chave: a chave não aparece; com ela, outra cor grava false
        assert_eq!(accent_auto_value(None, "#5294e2", &off, false), None);
        assert_eq!(accent_auto_value(None, "#5294e2", &off, true), Some(false));
    }

    #[test]
    fn split_tables_like_python() {
        // Os casos do ConfigValuesTests.test_split_tables do tests/test_sway_theme.py
        assert_eq!(split_tables(""), (String::new(), ""));
        assert_eq!(split_tables("a = 1\n# fim\n"), ("a = 1\n# fim\n".to_string(), ""));
        assert_eq!(
            split_tables("a = 1\n\n# fx\n[effects]\nblur = true\n"),
            ("a = 1\n\n".to_string(), "# fx\n[effects]\nblur = true\n")
        );
        assert_eq!(split_tables("[effects]\n"), (String::new(), "[effects]\n"));
    }

    #[test]
    fn new_key_goes_before_the_effects_table() {
        let dir = std::env::temp_dir().join(format!("swaydesk-set-config-effects-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        let template = include_str!("../../../home-copy/.config/sway-desktop/config.toml");
        std::fs::write(&file, template.replace("folder_color = \"\"\n", "")).unwrap();
        set_config_values(&file, &[("folder_color", "teal".into()), ("accent", "#112233".into())]).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let (config, warnings) = swaydesk_common::config::parse_config(&text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!((config.folder_color.as_str(), config.accent.as_str()), ("teal", "#112233"));
        assert_eq!(config.effects, swaydesk_common::config::Effects::default());
        assert!(config.disabled.is_empty());
        // A chave nova fica antes dos comentários da primeira tabela ([enabled]), separada por uma linha em branco
        let (head, tail) = split_tables(&text);
        assert!(head.ends_with("folder_color = \"teal\"\n\n"), "{head}");
        assert!(tail.starts_with("# Componentes da sessão"), "{tail}");
        assert!(tail.contains("\n[effects]\n"), "{tail}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn foreign_desktop_only_outside_our_compositors() {
        assert!(foreign_desktop_note("GNOME", "").unwrap().contains("GNOME"));
        assert!(foreign_desktop_note("ubuntu:GNOME", "").unwrap().contains("ubuntu:GNOME"));
        assert!(foreign_desktop_note("KDE", "").is_some());
        for (desktop, sock) in [("sway", ""), ("Hyprland", ""), ("river", ""), ("niri", ""), ("", ""), ("KDE", "/run/user/1000/sway-ipc.sock")] {
            assert_eq!(foreign_desktop_note(desktop, sock), None, "{desktop} {sock}");
        }
    }

    #[test]
    fn color_order() {
        let css = "@define-color selected_bg_color #111111;\n@define-color theme_selected_bg_color #222222;\n@define-color accent_color #333333;\n";
        assert_eq!(accent_of(css).as_deref(), Some("#333333"));
        assert_eq!(accent_of(&css.replace("accent_color #333333", "x #333333")).as_deref(), Some("#222222"));
        assert_eq!(accent_of("@define-color selected_bg_color #ABCDEF;").as_deref(), Some("#abcdef"));
        assert_eq!(accent_of("button { color: red; }"), None);
    }

    #[test]
    fn dark_from_colors() {
        let css = |bg: &str, fg: Option<&str>| {
            format!("@define-color theme_bg_color {bg};\n{}", fg.map(|f| format!("@define-color theme_fg_color {f};\n")).unwrap_or_default())
        };
        assert!(dark_of("Cinnamox-Kashmir-Blue", &css("#577584", Some("#daeaf2"))));
        assert!(!dark_of("Falso-Dark", &css("#f0f0f0", Some("#303030"))));
        assert!(dark_of("Mint-Y-Dark", &css("#2e2e33", None)));
        assert!(!dark_of("Mint-Y", &css("#f8f8f9", None)));
        assert!(dark_of("Adwaita-dark", "button { color: red; }"));
    }

    #[test]
    fn dark_suffixes_leave_the_name() {
        for (theme, name) in [("Nome-Dark-Red", "Nome-Red"), ("Nome-Darker-Red", "Nome-Red"), ("Nome-Dark", "Nome"), ("Nome-dark", "Nome"),
            ("Nome", "Nome"), ("Nome-Darkish", "Nome-Darkish"), ("A-Dark-Dark", "A")] {
            assert_eq!(without_dark(theme), name, "{theme}");
        }
    }
}
