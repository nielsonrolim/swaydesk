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

use crate::desktop::{self as dc, ICONS_WRAPPER};
use crate::families::{self as sf, Style};
use crate::papirus as sp;
use crate::{fill, fill_one, run, run_timeout, Error};

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

pub fn state_dir() -> PathBuf {
    match std::env::var_os("XDG_STATE_HOME").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => dc::home().join(".local/state"),
    }
    .join("sway-desktop")
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

/// Troca só estas linhas do config.toml: os comentários e o resto ficam como estão.
pub fn set_config_values(file: &Path, values: &[(&str, String)]) -> Result<(), Error> {
    let mut text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err.into()),
    };
    check_toml(&text)?; // um arquivo inválido é do usuário: não muda nada
    for (key, value) in values {
        let line = format!("{key} = {}", serde_json::to_string(value).unwrap_or_default());
        let pattern = Regex::new(&format!(r"(?m)^{}\s*=.*$", regex::escape(key))).expect("key pattern");
        if pattern.is_match(&text) {
            text = pattern.replacen(&text, 1, regex::NoExpand(&line)).into_owned();
        } else {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&line);
            text.push('\n');
        }
    }
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
/// instalado, ele herda o escolhido e o Breeze Dark.
pub fn gtk_icon_theme(name: &str, folders: Option<&str>, remove: bool) -> String {
    if ["breeze", FALLBACK_ICONS, ICONS_WRAPPER].contains(&name) {
        return name.to_string();
    }
    let colored = if sp::is_papirus(name) { folders.filter(|f| !f.is_empty()) } else { None };
    let fallback = dc::icons_installed(FALLBACK_ICONS);
    if colored.is_none() && !fallback {
        if remove {
            sp::remove();
        }
        return name.to_string();
    }
    let mut inherits = vec![name.to_string()];
    if fallback {
        inherits.push(FALLBACK_ICONS.to_string());
    }
    inherits.push("hicolor".to_string());
    match sp::build(name, &inherits, colored) {
        Ok(()) => ICONS_WRAPPER.to_string(),
        Err(_) => name.to_string(),
    }
}

/// Gera (ou apaga) o tema dos ícones da config e devolve o nome a gravar no dconf.
pub fn config_icon_theme(config: &Config, remove: bool) -> String {
    gtk_icon_theme(&dc::installed_icon_theme(config), folder_color(config).as_deref(), remove)
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

/// O DCONF_PROFILE do Sway em execução deste usuário ("" se ele não o tem), ou None sem Sway à vista.
/// Serve a um ambiente que tem o XDG_CURRENT_DESKTOP=sway mas não o DCONF_PROFILE (um shell de fora da
/// sessão que exportou um e não o outro). Primeiro o pid do SWAYSOCK; se ele não for um sway nosso (um
/// socket velho), os processos sway, do menor pid. SWAY_DESKTOP_PROC troca o /proc nos testes.
pub fn running_sway_profile() -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let proc = std::env::var_os("SWAY_DESKTOP_PROC").filter(|v| !v.is_empty()).map_or_else(|| PathBuf::from("/proc"), PathBuf::from);
    let mut scanned: Vec<u64> = std::fs::read_dir(&proc)
        .map(|dir| dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect())
        .unwrap_or_default();
    scanned.sort_unstable();
    let pids = std::env::var("SWAYSOCK")
        .ok()
        .as_deref()
        .and_then(swaysock_pid)
        .into_iter()
        .chain(scanned.into_iter().map(|p| p.to_string()));
    // SAFETY: getuid não falha nem toca memória
    let uid = unsafe { libc::getuid() };
    for pid in pids {
        let dir = proc.join(&pid);
        let is_ours = std::fs::read_to_string(dir.join("comm")).is_ok_and(|c| c.trim() == "sway")
            && std::fs::metadata(&dir).is_ok_and(|m| m.uid() == uid);
        let Some(environ) = is_ours.then(|| std::fs::read(dir.join("environ")).ok()).flatten() else {
            continue;
        };
        let value = environ
            .split(|b| *b == 0)
            .find_map(|item| item.strip_prefix(b"DCONF_PROFILE="))
            .map(|v| String::from_utf8_lossy(v).into_owned());
        return Some(value.unwrap_or_default());
    }
    None
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

/// Se a sessão do Sway lê o banco próprio. Quem o liga é o sway-session dos dotfiles, que exporta o
/// DCONF_PROFILE quando ~/.local/share/sway-desktop/dconf/sway existe. Só com os pacotes, nada o exporta: os
/// apps leem a base normal, e gravar no banco do Sway não mudaria nada (visto na VM, Sway aberto pelo tty).
fn sway_profile_in_use() -> bool {
    std::env::var_os("DCONF_PROFILE").is_some_and(|v| !v.is_empty())
        || config::data_home().join("sway-desktop/dconf/sway").is_file()
}

/// O DCONF_PROFILE da base do Sway, de onde quer que se rode. Numa sessão antiga, ou sem o banco próprio em
/// uso (só os pacotes), nenhum.
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

/// Em Wayland, o GTK lê o tema daqui, pelo portal.
pub fn set_dconf(config: &Config, icons: Option<String>) -> bool {
    write_dconf(&dconf_values(config, icons))
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
    if !dconf_is_unset("gtk-theme") {
        return refresh_icons(config);
    }
    let mut values = dconf_values(config, None);
    // O mesmo do seat do Sway (00-cursor.conf): sem isso, o cursor mudaria ao passar por cima dos apps GTK
    if let Some(cursor) = swaydesk_common::cursor::cursor_theme() {
        values.push(("cursor-theme", cursor.to_string()));
    }
    let unset: Vec<(&str, String)> = values.into_iter().filter(|(key, _)| dconf_is_unset(key)).collect();
    write_dconf(&unset)
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
    run(&["pkill", "-USR1", "-x", "kitty"], None);
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
    run(&["pkill", "-x", name], None);
    for _ in 0..30 {
        if run(&["pgrep", "-x", name], None) != 0 {
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
    if let Some(command) = command.filter(|_| run(&["pgrep", "-x", "waybar"], None) == 0) {
        stop("waybar");
        if run(&["swaymsg", "exec", &command], None) != 0 {
            failures.push("Waybar".to_string());
        } else {
            wait_for_tray();
        }
    }
    if run(&["pgrep", "-x", "sway-drawer"], None) == 0 {
        // O lançador dos dotfiles ou, instalada por pacote, o swaydesk-drawer
        let drawer = swaydesk_common::paths::component("sway-drawer", "swaydesk-drawer");
        let start = format!("{} start", shell_quote(&drawer));
        if !stop("sway-drawer") || run(&["swaymsg", "exec", &start], None) != 0 {
            failures.push(tr("drawer"));
        }
    }
    for name in ["sway-dock", "sway-osd-bar"] {
        if in_session_loop(name).is_some() {
            run(&["pkill", "-x", name], None); // o laço do config do Sway o abre de novo
        }
    }
    // SAFETY: o getuid não falha
    let uid = unsafe { libc::getuid() }.to_string();
    if run(&["pgrep", "-u", &uid, "-x", "wob"], None) == 0 {
        failures.push(tr("volume indicator (log in again)"));
    }
    failures
}

/// Os ícones que o apply grava para o tema.
pub fn chosen_icons(chosen: &Theme, current: &Config, folders: Option<&str>) -> String {
    let papirus = sf::icons_for_style(sf::style_of(chosen));
    if (folders.is_some() || sp::is_papirus(&current.icon_theme)) && dc::icons_installed(papirus) {
        return papirus.to_string();
    }
    chosen.icons.clone().unwrap_or_else(|| dc::installed_icon_theme(current))
}

fn check_folders(folders: Option<&str>) -> Result<(), Error> {
    match folders {
        Some(color) if papirus_color(color).is_none() => {
            Err(Error::Value(fill(&tr("unknown folder color: %(color)s. See them with: sway-theme colors"), &[("color", color)])))
        }
        _ => Ok(()),
    }
}

pub struct Applied {
    pub notes: Vec<String>,
    pub failures: Vec<String>,
}

/// Aplica a variante da família no estilo, com a cor do Papirus. Devolve o tema e o resultado.
pub fn pick(family: &str, style: Style, folders: &str) -> Result<(String, Applied), Error> {
    check_folders(Some(folders))?;
    let themes = discover();
    let (families, _others) = sf::group(&themes);
    let Some(found) = families.iter().find(|f| fold(&f.name) == fold(family)) else {
        return Err(Error::Value(fill_one(&tr("theme family not installed: %s. See them with: sway-theme families"), family)));
    };
    let accent = papirus_color(folders).unwrap_or_default();
    let Some(theme) = sf::resolve(found, style, accent, folders) else {
        return Err(Error::Value(fill(&tr("%(family)s has no %(style)s version"), &[("family", &found.name), ("style", &style.label())])));
    };
    let applied = apply(&theme, Some(accent), Some(folders), Some(themes))?;
    Ok((theme, applied))
}

/// Aplica tudo. Erra antes de mudar qualquer coisa. folders: a cor das pastas do Papirus, só delas.
pub fn apply(theme: &str, accent: Option<&str>, folders: Option<&str>, themes: Option<Vec<Theme>>) -> Result<Applied, Error> {
    let available = themes.unwrap_or_else(discover);
    let Some(chosen) = available.iter().find(|t| t.name == theme).cloned() else {
        return Err(Error::Value(fill_one(&tr("theme not installed: %s. See them with: sway-theme list"), theme)));
    };
    if let Some(accent) = accent.filter(|a| !is_accent(a)) {
        return Err(Error::Value(fill(&tr("invalid color: %(color)s (use one like %(example)s)"), &[("color", accent), ("example", DEFAULT_ACCENT)])));
    }
    check_folders(folders)?;
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
    let icons = chosen_icons(&chosen, &current, folders);
    if chosen.icons.is_none() && icons != sf::icons_for_style(sf::style_of(&chosen)) {
        notes.push(fill(&tr("There are no icons for %(theme)s: kept %(icons)s"), &[("theme", theme), ("icons", &icons)]));
    }
    let folder_value = folders.map_or_else(|| current.folder_color.clone(), str::to_string);
    set_config_values(&file, &[("gtk_theme", theme.to_string()), ("icon_theme", icons), ("accent", color), ("folder_color", folder_value)])?;
    let config = load_config(&file).0;
    let colors = config::palette(&config);
    // Antes de gerar os settings.ini e o qt5ct: eles usam o tema gerado, se ele existir
    let icon_name = config_icon_theme(&config, true);
    if dc::generate(&config)?.iter().any(|name| name.ends_with("settings.ini")) {
        notes.push(tr("The GTK settings.ini files belong to another desktop (KDE, for example) and were left as they were: the X11 apps in Sway follow their theme"));
    }
    if let Some(note) = kde_colors_note(theme) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
