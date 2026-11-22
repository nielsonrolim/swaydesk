//! A config do desktop (`~/.config/sway-desktop/config.toml`), as cores e as fontes: o porte do que
//! os componentes usam de `desktop_config.py`. Um valor ruim vira aviso e não invalida os outros.
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

pub const DEFAULT_ACCENT: &str = "#199ca8";
/// Abaixo disso, texto preto: todas as cores do Mint-Y ficam com branco, como nos temas.
const MIN_WHITE_CONTRAST: f64 = 2.4;
pub const TEXT_FONT: &str = "Noto Sans";
pub const ICON_FONT: &str = "Symbols Nerd Font";
// As mesmas chaves do Config do desktop_config.py
const KNOWN: &[&str] = &[
    "locale", "weather_location", "holidays", "accent", "accent_auto", "gtk_theme", "icon_theme", "folder_color", "file_manager",
    "components", "enabled", "effects", "shortcuts", "power_profiles_in_system",
];
/// As chaves do [enabled]: os componentes da sessão que podem ser desligados (o ENABLED_KEYS do Python).
pub const ENABLED_KEYS: &[&str] = &[
    "dock", "app_grid", "switcher", "notifications", "osd", "clipboard_history", "nightlight", "bing_wallpaper",
    "polkit_agent", "update_notifier", "displays", "autostart",
];
/// As do [enabled.bar], com o módulo da Waybar de cada uma (o BAR_MODULES do Python). Os workspaces, o
/// sway/mode e a bandeja ficam sempre.
pub const BAR_MODULES: &[(&str, &str)] = &[
    ("weather", "custom/weather"), ("clock", "custom/clock"), ("media", "custom/media"), ("recording", "custom/rec"),
    ("notifications", "custom/notification"), ("clipboard", "custom/clipboard"), ("language", "custom/language"),
    ("system", "custom/system"), ("brightness", "custom/brightness"), ("caffeine", "custom/caffeine"),
    ("bluetooth", "custom/bluetooth"), ("network", "custom/network"), ("volume", "custom/volume"),
    ("battery", "custom/battery"), ("power", "custom/power"), ("updates", "custom/updates"),
];
// As chaves do [effects] (o Effects do desktop_config.py)
const EFFECT_KEYS: &[&str] = &["corner_radius", "blur", "shadows", "layer_blur", "dim_inactive", "animation_duration_ms"];
/// O maior corner_radius aceito: mais que isso, os cantos comem o conteúdo das janelas
pub const MAX_CORNER_RADIUS: i64 = 30;
/// O limite do animation_duration_ms do SwayFX (5 segundos)
pub const MAX_ANIMATION_DURATION_MS: i64 = 5000;

/// As cores do papirus-folders, com a cor da frente da pasta (48x48/places/folder-<cor>.svg). A adwaita e
/// a yaru são o destaque padrão desses temas, e não a cor da pasta. Na ordem do desktop_config.py.
pub const PAPIRUS_COLORS: &[(&str, &str)] = &[
    ("adwaita", "#3584e4"), ("black", "#4f4f4f"), ("blue", "#5294e2"), ("bluegrey", "#607d8b"), ("breeze", "#57b8ec"),
    ("brown", "#ae8e6c"), ("carmine", "#a30002"), ("cyan", "#00bcd4"), ("darkcyan", "#45abb7"), ("deeporange", "#eb6637"),
    ("green", "#87b158"), ("grey", "#8e8e8e"), ("indigo", "#5c6bc0"), ("magenta", "#ca71df"), ("nordic", "#81a1c1"),
    ("orange", "#ee923a"), ("palebrown", "#d1bfae"), ("paleorange", "#eeca8f"), ("pink", "#f06292"), ("red", "#e25252"),
    ("teal", "#16a085"), ("violet", "#7e57c2"), ("white", "#e4e4e4"), ("yaru", "#e95420"), ("yellow", "#f9bd30"),
];
pub const COMPONENTS: &[&str] = &["auto", "python", "rust"];
/// Os perfis de atalhos do shortcuts (o SHORTCUT_PROFILES do Python): o nome no config e o nome mostrado, o da
/// marca, sem tradução. Cada um é um sway/keys/<nome>.conf, incluído depois do keys/base.conf.
pub const SHORTCUT_PROFILES: &[(&str, &str)] =
    &[("swaydesk", "Swaydesk"), ("i3", "i3"), ("hyprland", "Hyprland"), ("niri", "niri"), ("dwm", "dwm")];
/// A pasta dos perfis no pacote swaydesk-desktop (a do config do Sway da sessão do pacote)
pub const PACKAGE_KEYS_DIR: &str = "/usr/share/swaydesk-desktop/sway/keys";

/// O hex de uma cor do Papirus.
pub fn papirus_color(name: &str) -> Option<&'static str> {
    PAPIRUS_COLORS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Como "pt_BR"; vazio: o do ambiente
    pub locale: String,
    /// Como "#199ca8", em minúsculas
    pub accent: String,
    /// A cor de destaque tirada do papel de parede: o sway-theme accent-auto grava a cor em accent a cada troca.
    /// Sem a chave (ou com um valor inválido), ligado só se o accent é o DEFAULT_ACCENT: quem escolheu outra cor
    /// antes da chave existir fica com ela
    pub accent_auto: bool,
    /// O código do país dos feriados do calendário, como "BR"; vazio: o do locale; "none": nenhum
    pub holidays: String,
    /// A cidade do tempo, como "São Paulo"; vazio: o wttr.in usa a do IP
    pub weather_location: String,
    pub gtk_theme: String,
    pub icon_theme: String,
    /// A cor das pastas do Papirus, como "teal"; vazio: a mais perto do destaque
    pub folder_color: String,
    /// O comando, como "thunar"; vazio: o primeiro instalado
    pub file_manager: String,
    /// A versão dos componentes: auto (Rust onde houver), python ou rust
    pub components: String,
    /// O perfil de atalhos, um dos SHORTCUT_PROFILES; "swaydesk" sem a chave ou com um nome inválido
    pub shortcuts: String,
    /// Quando o popup Sistema mostra os perfis de energia, um dos power::IN_SYSTEM: "auto" (só sem bateria),
    /// "always" ou "never"; "auto" sem a chave ou com um valor inválido
    pub power_profiles_in_system: String,
    /// Os componentes desligados no [enabled] (false), como "dock" ou "bar.weather". O que não está aqui
    /// está ligado: a chave ausente liga, para um config antigo continuar como era
    pub disabled: BTreeSet<String>,
    /// Os efeitos do SwayFX ([effects]); o Sway puro os ignora
    pub effects: Effects,
}

impl Config {
    /// Se o componente está ligado. Um nome desconhecido conta como ligado.
    pub fn enabled(&self, name: &str) -> bool {
        !self.disabled.contains(name)
    }
}

/// Se o nome é uma das chaves do [enabled] ("dock") ou do [enabled.bar] ("bar.weather").
pub fn known_component(name: &str) -> bool {
    ENABLED_KEYS.contains(&name) || name.strip_prefix("bar.").is_some_and(|m| BAR_MODULES.iter().any(|(k, _)| *k == m))
}

/// Os nomes desligados do [enabled]. Um valor que não é true nem false, ou um nome desconhecido, avisa e
/// deixa ligado: um erro no arquivo nunca desliga nada. O parse_enabled do Python.
fn parse_enabled(value: &toml::Value, warnings: &mut Vec<String>) -> BTreeSet<String> {
    let mut off = BTreeSet::new();
    let Some(table) = value.as_table() else {
        warnings.push("config: invalid value in enabled".to_string());
        return off;
    };
    for (key, value) in table {
        if let (true, Some(bar)) = (key == "bar", value.as_table()) {
            for (module, on) in bar {
                if !BAR_MODULES.iter().any(|(k, _)| k == module) {
                    warnings.push(format!("config: unknown option: enabled.bar.{module}"));
                } else if let Some(on) = on.as_bool() {
                    if !on {
                        off.insert(format!("bar.{module}"));
                    }
                } else {
                    warnings.push(format!("config: invalid value in enabled.bar.{module} (use true or false)"));
                }
            }
        } else if !ENABLED_KEYS.contains(&key.as_str()) {
            warnings.push(format!("config: unknown option: enabled.{key}"));
        } else if let Some(on) = value.as_bool() {
            if !on {
                off.insert(key.clone());
            }
        } else {
            warnings.push(format!("config: invalid value in enabled.{key} (use true or false)"));
        }
    }
    off
}

/// Os efeitos do SwayFX, do [effects] da config. Cantos, sombras e o blur das janelas vêm ligados e discretos; o
/// blur das camadas vem desligado (na VM, com virgl, ele deixou lixo visual na barra e nos popups) até um teste
/// numa GPU real. Cada efeito liga ou desliga sozinho, e uma chave ausente fica com o padrão.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effects {
    /// Os cantos arredondados das janelas e das camadas, em pixels; 0 desliga
    pub corner_radius: u32,
    /// O blur atrás das janelas transparentes
    pub blur: bool,
    /// A sombra das janelas
    pub shadows: bool,
    /// O blur atrás da Waybar, da dock, da grade, dos popups, das notificações, do OSD e do switcher; desligado
    /// por padrão
    pub layer_blur: bool,
    /// O escurecimento das janelas sem o foco, em centésimos (0.15 na config: 15); 0 desliga
    pub dim_inactive: u32,
    /// A duração das animações do SwayFX 0.6 (abrir e fechar janelas), em milissegundos; 0 (o padrão) desliga
    pub animation_duration_ms: u32,
}

impl Default for Effects {
    fn default() -> Self {
        Effects { corner_radius: 10, blur: true, shadows: true, layer_blur: false, dim_inactive: 0, animation_duration_ms: 0 }
    }
}

/// O [effects] da config, com os avisos de cada chave ruim (as outras valem).
fn parse_effects(table: &toml::Table, warnings: &mut Vec<String>) -> Effects {
    let mut effects = Effects::default();
    for (key, value) in table {
        if !EFFECT_KEYS.contains(&key.as_str()) {
            warnings.push(format!("config: unknown option: effects.{key}"));
            continue;
        }
        match (key.as_str(), value) {
            ("corner_radius", toml::Value::Integer(n)) if (0..=MAX_CORNER_RADIUS).contains(n) => {
                effects.corner_radius = *n as u32
            }
            ("corner_radius", _) => {
                warnings.push(format!("config: invalid effects.corner_radius (use 0 to {MAX_CORNER_RADIUS})"))
            }
            ("blur", toml::Value::Boolean(on)) => effects.blur = *on,
            ("shadows", toml::Value::Boolean(on)) => effects.shadows = *on,
            ("layer_blur", toml::Value::Boolean(on)) => effects.layer_blur = *on,
            ("blur" | "shadows" | "layer_blur", _) => {
                warnings.push(format!("config: invalid effects.{key} (use true or false)"))
            }
            // 0 a 1, em float ou inteiro (0 e 1); guardado em centésimos, como o Python
            ("dim_inactive", toml::Value::Float(x)) if (0.0..=1.0).contains(x) => {
                effects.dim_inactive = (x * 100.0).round() as u32
            }
            ("dim_inactive", toml::Value::Integer(n)) if (0..=1).contains(n) => effects.dim_inactive = *n as u32 * 100,
            ("dim_inactive", _) => warnings.push("config: invalid effects.dim_inactive (use 0.0 to 1.0)".to_string()),
            ("animation_duration_ms", toml::Value::Integer(n)) if (0..=MAX_ANIMATION_DURATION_MS).contains(n) => {
                effects.animation_duration_ms = *n as u32
            }
            ("animation_duration_ms", _) => warnings.push(format!(
                "config: invalid effects.animation_duration_ms (use 0 to {MAX_ANIMATION_DURATION_MS})"
            )),
            _ => {}
        }
    }
    effects
}

impl Default for Config {
    fn default() -> Self {
        Config {
            locale: String::new(),
            accent: DEFAULT_ACCENT.to_string(),
            accent_auto: true,
            holidays: String::new(),
            weather_location: String::new(),
            gtk_theme: "Mint-Y-Dark-Teal".to_string(),
            icon_theme: "Papirus-Dark".to_string(),
            folder_color: String::new(),
            file_manager: String::new(),
            components: "auto".to_string(),
            shortcuts: "swaydesk".to_string(),
            power_profiles_in_system: "auto".to_string(),
            disabled: BTreeSet::new(),
            effects: Effects::default(),
        }
    }
}

/// O `[\w .+-]+` (THEME_NAME) e, sem o espaço, o `[\w.+-]+` (COMMAND_NAME) do Python.
fn is_name(value: &str, space: bool) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_alphanumeric() || "_.+-".contains(c) || (space && c == ' '))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    pub accent: String,
    /// O destaque sob o mouse
    pub hover: String,
    /// O indicador de onde a próxima janela abre, no Sway
    pub bright: String,
    /// Texto secundário sobre o destaque
    pub soft: String,
    /// Texto sobre o destaque
    pub on_accent: String,
}

fn env_dir(name: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(name) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home().join(fallback),
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// A pasta dos perfis de atalhos desta sessão (o keys_dir do Python). Na sessão do pacote, a do pacote. Nos
/// dotfiles, a pasta keys ao lado do config do Sway de verdade (o link ~/.config/sway/config resolvido, no
/// repositório): vale mesmo sem os links do install.sh, depois de um git pull. Sem ela, a de ~/.config/sway/keys
/// ou a do pacote, a primeira que tiver o base.conf.
pub fn keys_dir() -> PathBuf {
    if crate::paths::package_session() {
        return PathBuf::from(PACKAGE_KEYS_DIR);
    }
    let sway = env_dir("XDG_CONFIG_HOME", ".config").join("sway");
    let mut candidates = Vec::new();
    if let Some(real) = std::fs::canonicalize(sway.join("config")).ok().and_then(|p| p.parent().map(|d| d.join("keys"))) {
        candidates.push(real);
    }
    candidates.push(sway.join("keys"));
    candidates.push(PathBuf::from(PACKAGE_KEYS_DIR));
    let first = candidates[0].clone();
    candidates.into_iter().find(|d| d.join("base.conf").is_file()).unwrap_or(first)
}

/// O arquivo de atalhos de um perfil (`<keys_dir>/<perfil>.conf`), se existir. Um nome que não é dos
/// SHORTCUT_PROFILES dá None.
pub fn shortcut_keys_file(profile: &str) -> Option<PathBuf> {
    if !SHORTCUT_PROFILES.iter().any(|(id, _)| *id == profile) {
        return None;
    }
    Some(keys_dir().join(format!("{profile}.conf"))).filter(|p| p.is_file())
}

pub fn path() -> PathBuf {
    env_dir("XDG_CONFIG_HOME", ".config").join("sway-desktop").join("config.toml")
}

pub fn data_home() -> PathBuf {
    env_dir("XDG_DATA_HOME", ".local/share")
}

pub fn cache_home() -> PathBuf {
    env_dir("XDG_CACHE_HOME", ".cache")
}

fn is_locale(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 5 && b[..2].iter().all(u8::is_ascii_lowercase) && b[2] == b'_' && b[3..].iter().all(u8::is_ascii_uppercase)
}

/// O `#[0-9a-fA-F]{6}` (ACCENT) do Python.
pub fn is_accent(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|c| c.is_ascii_hexdigit())
}

/// Devolve a config e os avisos.
pub fn parse_config(text: &str) -> (Config, Vec<String>) {
    let mut config = Config::default();
    let mut warnings = Vec::new();
    let mut auto_given = false;
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => return (config, vec![format!("config: could not read: {}", err.message())]),
    };
    for (key, value) in &table {
        if !KNOWN.contains(&key.as_str()) {
            warnings.push(format!("config: unknown option: {key}"));
            continue;
        }
        if key == "enabled" {
            config.disabled = parse_enabled(value, &mut warnings);
            continue;
        }
        if key == "effects" {
            match value.as_table() {
                Some(effects) => config.effects = parse_effects(effects, &mut warnings),
                None => warnings.push("config: invalid value in effects (use an [effects] table)".to_string()),
            }
            continue;
        }
        if key == "accent_auto" {
            match value.as_bool() {
                Some(auto) => {
                    config.accent_auto = auto;
                    auto_given = true;
                }
                None => warnings.push("config: invalid value in accent_auto (use true or false)".to_string()),
            }
            continue;
        }
        let Some(value) = value.as_str() else {
            warnings.push(format!("config: invalid value in {key}"));
            continue;
        };
        match key.as_str() {
            "locale" if !value.is_empty() && !is_locale(value) => {
                warnings.push(format!("config: invalid locale: {value} (use one like pt_BR)"))
            }
            "locale" => config.locale = value.to_string(),
            "accent" if !is_accent(value) => {
                warnings.push(format!("config: invalid accent color: {value} (use one like {DEFAULT_ACCENT})"))
            }
            "accent" => config.accent = value.to_lowercase(),
            // Como o HOLIDAYS do desktop_config.py: duas maiúsculas, "none" ou vazio
            "holidays"
                if !(value.is_empty() || value == "none" || (value.len() == 2 && value.bytes().all(|c| c.is_ascii_uppercase()))) =>
            {
                warnings.push(format!("config: invalid holidays: {value} (use a country code like BR, none, or empty)"))
            }
            "holidays" => config.holidays = value.to_string(),
            "weather_location" => config.weather_location = value.to_string(),
            "gtk_theme" | "icon_theme" if !is_name(value, true) => {
                warnings.push(format!("config: invalid theme name in {key}: {value}"))
            }
            "gtk_theme" => config.gtk_theme = value.to_string(),
            "icon_theme" => config.icon_theme = value.to_string(),
            "folder_color" if !value.is_empty() && papirus_color(value).is_none() => {
                warnings.push(format!("config: invalid folder color: {value} (use one of sway-theme colors, or empty)"))
            }
            "folder_color" => config.folder_color = value.to_string(),
            "components" if !COMPONENTS.contains(&value) => {
                warnings.push(format!("config: invalid components: {value} (use auto, python or rust)"))
            }
            "components" => config.components = value.to_string(),
            "shortcuts" if !SHORTCUT_PROFILES.iter().any(|(id, _)| *id == value) => warnings.push(format!(
                "config: invalid shortcuts: {value} (use swaydesk, i3, hyprland, niri or dwm)"
            )),
            "shortcuts" => config.shortcuts = value.to_string(),
            "power_profiles_in_system" if !crate::power::IN_SYSTEM.contains(&value) => warnings.push(format!(
                "config: invalid power_profiles_in_system: {value} (use auto, always or never)"
            )),
            "power_profiles_in_system" => config.power_profiles_in_system = value.to_string(),
            "file_manager" if !value.is_empty() && !is_name(value, false) => {
                warnings.push(format!("config: invalid file manager: {value} (use the command name, like thunar)"))
            }
            "file_manager" => config.file_manager = value.to_string(),
            _ => {}
        }
    }
    if !auto_given {
        config.accent_auto = config.accent == DEFAULT_ACCENT;
    }
    (config, warnings)
}

pub fn load_config(file: &Path) -> (Config, Vec<String>) {
    match std::fs::read_to_string(file) {
        Ok(text) => parse_config(&text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Config::default(), vec![]),
        Err(err) => (Config::default(), vec![format!("config: could not read {}: {err}", file.display())]),
    }
}

/// A config do usuário; os avisos vão para o stderr (o sway.log).
pub fn load() -> Config {
    let (config, warnings) = load_config(&path());
    for warning in warnings {
        eprintln!("{warning}");
    }
    config
}

/// O locale da config ou, sem ele, o de LC_ALL, LC_TIME ou LANG, nessa ordem (o primeiro que existir);
/// C e POSIX: vazio. O `effective_locale` do desktop_config.py.
pub fn effective_locale(config: &Config, env: &HashMap<String, String>) -> String {
    if !config.locale.is_empty() {
        return config.locale.clone();
    }
    for name in ["LC_ALL", "LC_TIME", "LANG"] {
        if let Some(value) = env.get(name).filter(|v| !v.is_empty()) {
            // O LOCALE.match do Python: "xx_YY" no começo
            let b = value.as_bytes();
            let ok = b.len() >= 5 && b[..2].iter().all(u8::is_ascii_lowercase) && b[2] == b'_' && b[3..5].iter().all(u8::is_ascii_uppercase);
            return if ok { value[..5].to_string() } else { String::new() };
        }
    }
    String::new()
}

/// Os locales da data, na ordem de tentar: o efetivo (o `locale` da config, LC_ALL, LC_TIME ou LANG) e, se
/// ele não estiver gerado, o da língua dos textos (`interface`, como "es_ES"). Só nomes xx_YY, sem repetir.
/// O C escolhido (LC_TIME=C.UTF-8 para a data ISO, LC_ALL=POSIX) fica no C: vazio; sem nada definido, a língua
/// dos textos.
pub fn time_locales(config: &Config, env: &HashMap<String, String>, interface: &str) -> Vec<String> {
    let mut out = Vec::new();
    let first = effective_locale(config, env);
    if !first.is_empty() {
        out.push(first);
    } else if ["LC_ALL", "LC_TIME", "LANG"].iter().any(|name| env.get(*name).is_some_and(|v| !v.is_empty())) {
        return out;
    }
    let b = interface.as_bytes();
    if b.len() >= 5 && b[..2].iter().all(u8::is_ascii_lowercase) && b[2] == b'_' && b[3..5].iter().all(u8::is_ascii_uppercase) {
        let interface = interface[..5].to_string();
        if !out.contains(&interface) {
            out.push(interface);
        }
    }
    out
}

/// O país dos feriados e se ele veio da config: (país, explícito). Sem o holidays na config, o país do
/// locale ("BR" em pt_BR); "none" desliga. C, POSIX ou nada: nenhum. O `holidays_country` do
/// desktop_config.py.
pub fn holidays_country(config: &Config, env: &HashMap<String, String>) -> (String, bool) {
    match config.holidays.as_str() {
        "none" => (String::new(), true),
        "" => (effective_locale(config, env).split_once('_').map(|(_, c)| c.to_string()).unwrap_or_default(), false),
        country => (country.to_string(), true),
    }
}

/// A parte do idioma: "pt" em "pt_BR".
pub fn language(config: &Config, env: &HashMap<String, String>) -> String {
    effective_locale(config, env).split('_').next().unwrap_or("").to_string()
}

/// As línguas dos textos, em ordem: a do config ou, sem ela, a do ambiente (LANGUAGE, LC_ALL,
/// LC_MESSAGES, LANG). C, POSIX ou nada: lista vazia, o inglês do código.
pub fn message_languages(config: &Config, env: &HashMap<String, String>) -> Vec<String> {
    if !config.locale.is_empty() {
        return vec![config.locale.clone()];
    }
    let get = |name: &str| env.get(name).filter(|v| !v.is_empty());
    if let Some(value) = get("LANGUAGE") {
        return value.split(':').filter(|n| !n.is_empty()).map(str::to_string).collect();
    }
    for name in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(value) = get(name) {
            let value = value.split('.').next().unwrap_or("").split('@').next().unwrap_or("");
            return if value == "C" || value == "POSIX" { vec![] } else { vec![value.to_string()] };
        }
    }
    vec![]
}

/// "#ff8000" -> [255, 128, 0]
pub fn channels(color: &str) -> [u8; 3] {
    let at = |i: usize| u8::from_str_radix(&color[i..i + 2], 16).unwrap_or(0);
    [at(1), at(3), at(5)]
}

/// A cor com uma parte (de 0 a 1) da outra.
pub fn mix(color: &str, other: &str, amount: f64) -> String {
    let (a, b) = (channels(color), channels(other));
    let mut out = String::from("#");
    for i in 0..3 {
        let value = round_half_even(a[i] as f64 + (b[i] as f64 - a[i] as f64) * amount);
        out.push_str(&format!("{value:02x}"));
    }
    out
}

/// O round do Python: o meio vai para o par.
fn round_half_even(value: f64) -> i64 {
    let floor = value.floor();
    let diff = value - floor;
    let base = floor as i64;
    if diff > 0.5 || (diff == 0.5 && base % 2 != 0) {
        base + 1
    } else {
        base
    }
}

/// Os canais sem a curva do sRGB, de 0 a 1: a base do contraste e do Lab.
pub fn linear_rgb(color: &str) -> [f64; 3] {
    channels(color).map(|c| {
        let v = c as f64 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}

/// Contraste do WCAG com o branco, de 1 a 21.
pub fn contrast_with_white(color: &str) -> f64 {
    let [r, g, b] = linear_rgb(color);
    1.05 / (0.2126 * r + 0.7152 * g + 0.0722 * b + 0.05)
}

/// Os tons do destaque. O padrão usa os tons do tema Mint-Y; as outras cores, tons calculados.
pub fn palette(config: &Config) -> Palette {
    let accent = config.accent.clone();
    if accent == DEFAULT_ACCENT {
        return Palette {
            accent,
            hover: "#22b3c0".into(),
            bright: "#29d0df".into(),
            soft: "#e0f4f6".into(),
            on_accent: "#ffffff".into(),
        };
    }
    let on_accent = if contrast_with_white(&accent) >= MIN_WHITE_CONTRAST { "#ffffff" } else { "#000000" };
    Palette {
        hover: mix(&accent, on_accent, 0.12),
        bright: mix(&accent, "#ffffff", 0.25),
        soft: mix(&accent, on_accent, 0.85),
        on_accent: on_accent.into(),
        accent,
    }
}

/// As cores com nome do CSS do GTK.
pub fn css_colors(colors: &Palette) -> String {
    format!(
        "@define-color accent {};\n@define-color accent_hover {};\n@define-color accent_soft {};\n\
         @define-color on_accent {};\n@define-color busy_sub #e0f4f6;\n",
        colors.accent, colors.hover, colors.soft, colors.on_accent
    )
}

/// As famílias, em ordem: a do texto, a do sistema, a dos ícones e a genérica.
pub fn font_families(system: Option<&str>) -> Vec<String> {
    let mut families: Vec<String> = Vec::new();
    for family in [Some(TEXT_FONT), system, Some(ICON_FONT), Some("sans-serif")].into_iter().flatten() {
        if !family.is_empty() && !families.iter().any(|f| f == family) {
            families.push(family.to_string());
        }
    }
    families
}

/// A regra que dá as fontes a tudo. Vem antes do resto.
pub fn font_css(system: Option<&str>) -> String {
    let names: Vec<String> = font_families(system)
        .into_iter()
        .map(|name| if name == "sans-serif" { name } else { format!("\"{name}\"") })
        .collect();
    format!("* {{ font-family: {}; }}\n", names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os mesmos casos do EnabledTests do tests/test_desktop_config.py (a paridade Rust = Python).
    const ENABLED_TOML: &str = "accent = \"#199ca8\"\n[enabled]\ndock = false\napp_grid = true\nosd = \"no\"\nfoo = true\n\
                                [enabled.bar]\nweather = false\nxyz = false\nclock = 1\n";

    #[test]
    fn enabled_reads_false_and_keeps_the_rest_on() {
        let (config, mut warnings) = parse_config(ENABLED_TOML);
        assert_eq!(config.disabled.iter().map(String::as_str).collect::<Vec<_>>(), ["bar.weather", "dock"]);
        // A tabela do toml vem em ordem alfabética, e a do Python na do arquivo: compara sem a ordem
        warnings.sort();
        assert_eq!(
            warnings,
            [
                "config: invalid value in enabled.bar.clock (use true or false)",
                "config: invalid value in enabled.osd (use true or false)",
                "config: unknown option: enabled.bar.xyz",
                "config: unknown option: enabled.foo",
            ]
        );
        assert!(!config.enabled("dock") && !config.enabled("bar.weather"));
        assert!(config.enabled("app_grid") && config.enabled("osd") && config.enabled("bar.clock"));
        assert!(config.enabled("no-such-thing"), "um nome desconhecido conta como ligado");
        assert_eq!(config.accent, "#199ca8", "o [enabled] não atrapalha as outras chaves");
    }

    #[test]
    fn enabled_absent_or_not_a_table_is_all_on() {
        assert!(parse_config("accent = \"#199ca8\"\n").0.disabled.is_empty());
        let (config, warnings) = parse_config("enabled = false\n");
        assert!(config.disabled.is_empty());
        assert_eq!(warnings, ["config: invalid value in enabled"]);
    }

    #[test]
    fn known_components() {
        assert!(known_component("dock") && known_component("bar.recording"));
        assert!(!known_component("bar.dock") && !known_component("weather") && !known_component("bar."));
    }

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn accent_auto_is_a_bool() {
        // Sem a chave: ligado com o destaque padrão, desligado com outra cor (escolhida antes da chave existir)
        assert!(parse_config("").0.accent_auto && Config::default().accent_auto);
        assert!(parse_config("accent = \"#199CA8\"\n").0.accent_auto);
        assert!(!parse_config("accent = \"#e25252\"\n").0.accent_auto);
        let (config, warnings) = parse_config("accent_auto = true\naccent = \"#e25252\"\n");
        assert!(config.accent_auto && warnings.is_empty());
        assert_eq!(config.accent, "#e25252");
        assert!(!parse_config("accent_auto = false\n").0.accent_auto);
        // Inválido: avisa e vale a regra de sem a chave
        for (text, auto) in [("accent_auto = \"yes\"\n", true), ("accent_auto = 1\naccent = \"#e25252\"\n", false)] {
            let (config, warnings) = parse_config(text);
            assert_eq!(config.accent_auto, auto, "{text}");
            assert_eq!(warnings, ["config: invalid value in accent_auto (use true or false)"]);
        }
    }

    #[test]
    fn bad_values_warn_and_keep_the_defaults() {
        let (config, warnings) = parse_config("accent = \"#ABCDEF\"\nlocale = \"pt-BR\"\nnope = \"x\"\ngtk_theme = \"Mint-Y\"\n");
        assert_eq!(config.accent, "#abcdef");
        assert_eq!(config.locale, "");
        assert_eq!(warnings.len(), 2);
        assert_eq!(parse_config("accent = 3").1, vec!["config: invalid value in accent"]);
        assert_eq!(parse_config("[[").0, Config::default());
    }

    #[test]
    fn effects_like_python() {
        // Os mesmos casos do EffectsTests do tests/test_desktop_config.py
        assert_eq!(
            Config::default().effects,
            Effects { corner_radius: 10, blur: true, shadows: true, layer_blur: false, dim_inactive: 0, animation_duration_ms: 0 }
        );
        assert_eq!(parse_config("[effects]\n"), (Config::default(), vec![]));
        let (config, warnings) = parse_config("[effects]\nblur = false\ndim_inactive = 0.15\n");
        assert_eq!(config.effects, Effects { blur: false, dim_inactive: 15, ..Effects::default() });
        assert!(warnings.is_empty());
        assert_eq!(parse_config("[effects]\ndim_inactive = 1\n").0.effects.dim_inactive, 100);
        assert_eq!(parse_config("[effects]\nanimation_duration_ms = 250\n").0.effects.animation_duration_ms, 250);
        assert_eq!(parse_config("[effects]\nanimation_duration_ms = 5000\n").0.effects.animation_duration_ms, 5000);
        for (text, warning) in [
            ("corner_radius = 31", "effects.corner_radius"),
            ("corner_radius = -1", "effects.corner_radius"),
            ("corner_radius = 2.5", "effects.corner_radius"),
            ("corner_radius = true", "effects.corner_radius"),
            ("blur = \"yes\"", "effects.blur"),
            ("shadows = 1", "effects.shadows"),
            ("dim_inactive = 1.5", "effects.dim_inactive"),
            ("dim_inactive = 2", "effects.dim_inactive"),
            ("dim_inactive = true", "effects.dim_inactive"),
            ("animation_duration_ms = 5001", "effects.animation_duration_ms"),
            ("animation_duration_ms = -1", "effects.animation_duration_ms"),
            ("animation_duration_ms = 0.5", "effects.animation_duration_ms"),
            ("animation_duration_ms = true", "effects.animation_duration_ms"),
            ("wobble = true", "unknown option: effects.wobble"),
        ] {
            let (config, warnings) = parse_config(&format!("[effects]\n{text}\n"));
            assert_eq!(config, Config::default(), "{text}");
            assert_eq!(warnings.len(), 1, "{text}");
            assert!(warnings[0].contains(warning), "{text}: {}", warnings[0]);
        }
        assert_eq!(parse_config("effects = \"on\"\n"), (Config::default(), vec!["config: invalid value in effects (use an [effects] table)".to_string()]));
    }

    #[test]
    fn effective_locale_and_language_like_python() {
        let env = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>();
        let mut config = Config::default();
        assert_eq!(effective_locale(&config, &env(&[("LANG", "pt_BR.UTF-8")])), "pt_BR");
        assert_eq!(effective_locale(&config, &env(&[("LC_TIME", "de_DE.UTF-8"), ("LANG", "pt_BR.UTF-8")])), "de_DE");
        assert_eq!(effective_locale(&config, &env(&[("LC_ALL", ""), ("LANG", "C.UTF-8")])), "");
        assert_eq!(language(&config, &env(&[("LANG", "es_ES.UTF-8")])), "es");
        config.locale = "fr_FR".into();
        assert_eq!(language(&config, &env(&[("LANG", "pt_BR.UTF-8")])), "fr");
        let (parsed, warnings) = parse_config("weather_location = \"São Paulo\"\n");
        assert_eq!((parsed.weather_location.as_str(), warnings.len()), ("São Paulo", 0));
    }

    #[test]
    fn the_date_follows_lc_time_then_the_interface_language() {
        let env = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>();
        let mixed = env(&[("LANG", "es_ES.UTF-8"), ("LC_TIME", "pt_BR.UTF-8")]);
        // A data no LC_TIME; sem ele gerado, a da língua dos textos
        assert_eq!(time_locales(&Config::default(), &mixed, "es_ES"), ["pt_BR", "es_ES"]);
        assert_eq!(time_locales(&Config::default(), &env(&[("LC_ALL", "de_DE.UTF-8"), ("LC_TIME", "pt_BR.UTF-8")]), "es_ES"), ["de_DE", "es_ES"]);
        // O `locale` da config manda nos textos e na data
        let fr = Config { locale: "fr_FR".into(), ..Config::default() };
        assert_eq!(time_locales(&fr, &mixed, "fr_FR"), ["fr_FR"]);
        // Sem repetir; a língua só com o idioma ("es", do LANGUAGE) ou o C não entram
        assert_eq!(time_locales(&Config::default(), &env(&[("LANG", "es_ES.UTF-8")]), "es_ES.UTF-8"), ["es_ES"]);
        assert_eq!(time_locales(&Config::default(), &env(&[("LANG", "C.UTF-8")]), "es"), Vec::<String>::new());
        // O C escolhido para a data fica no C, sem ir para a língua dos textos
        let none = Vec::<String>::new();
        assert_eq!(time_locales(&Config::default(), &env(&[("LANG", "es_ES.UTF-8"), ("LC_TIME", "C.UTF-8")]), "es_ES"), none);
        assert_eq!(time_locales(&Config::default(), &env(&[("LANG", "es_ES.UTF-8"), ("LC_ALL", "POSIX")]), "es_ES"), none);
        assert_eq!(time_locales(&Config::default(), &env(&[]), "pt_BR"), ["pt_BR"]);
    }

    #[test]
    fn holidays_country_like_python() {
        let env = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>();
        let pt = Config { locale: "pt_BR".into(), ..Config::default() };
        assert_eq!(holidays_country(&pt, &env(&[])), ("BR".to_string(), false));
        assert_eq!(holidays_country(&Config::default(), &env(&[("LANG", "en_US.UTF-8")])), ("US".to_string(), false));
        assert_eq!(holidays_country(&Config::default(), &env(&[("LANG", "C.UTF-8")])), (String::new(), false));
        let ar = Config { holidays: "AR".into(), ..pt.clone() };
        assert_eq!(holidays_country(&ar, &env(&[])), ("AR".to_string(), true));
        let none = Config { holidays: "none".into(), ..pt.clone() };
        assert_eq!(holidays_country(&none, &env(&[])), (String::new(), true));
        assert_eq!(parse_config("holidays = \"none\"").0.holidays, "none");
    }

    #[test]
    fn languages_follow_config_then_environment() {
        let pt = Config { locale: "pt_BR".into(), ..Config::default() };
        assert_eq!(message_languages(&pt, &env(&[("LANG", "en_US.UTF-8")])), ["pt_BR"]);
        let none = Config::default();
        assert_eq!(message_languages(&none, &env(&[("LANGUAGE", "fr:de"), ("LANG", "pt_BR")])), ["fr", "de"]);
        assert_eq!(message_languages(&none, &env(&[("LANG", "pt_BR.UTF-8@x")])), ["pt_BR"]);
        assert!(message_languages(&none, &env(&[("LC_ALL", "C.UTF-8")])).is_empty());
        assert!(message_languages(&none, &env(&[])).is_empty());
    }

    // Os valores conferidos com o desktop_config.py
    #[test]
    fn palette_matches_the_python_one() {
        let default = palette(&Config::default());
        assert_eq!(default.hover, "#22b3c0");
        let yellow = palette(&Config { accent: "#f5c211".into(), ..Config::default() });
        assert_eq!(
            (yellow.hover.as_str(), yellow.bright.as_str(), yellow.soft.as_str(), yellow.on_accent.as_str()),
            ("#d8ab0f", "#f8d14c", "#251d03", "#000000")
        );
        let blue = palette(&Config { accent: "#1a5fb4".into(), ..Config::default() });
        assert_eq!((blue.hover.as_str(), blue.on_accent.as_str()), ("#3572bd", "#ffffff"));
    }

    #[test]
    fn fonts_without_repeats() {
        assert_eq!(font_css(Some("Ubuntu")), "* { font-family: \"Noto Sans\", \"Ubuntu\", \"Symbols Nerd Font\", sans-serif; }\n");
        assert_eq!(font_families(Some("Noto Sans")), ["Noto Sans", "Symbols Nerd Font", "sans-serif"]);
    }

    #[test]
    fn theme_keys_like_python() {
        let (config, warnings) = parse_config(
            "gtk_theme = \"Mint-Y-Dark-Red\"\nicon_theme = \"Papirus\"\nfolder_color = \"teal\"\nfile_manager = \"nemo\"\ncomponents = \"python\"\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!((config.gtk_theme.as_str(), config.icon_theme.as_str(), config.folder_color.as_str()), ("Mint-Y-Dark-Red", "Papirus", "teal"));
        assert_eq!((config.file_manager.as_str(), config.components.as_str()), ("nemo", "python"));
        let (config, warnings) = parse_config(
            "gtk_theme = \"a/b\"\nfolder_color = \"roxo\"\ncomponents = \"go\"\nfile_manager = \"rm -rf\"\n");
        assert_eq!(warnings.len(), 4);
        assert_eq!(config, Config::default());
        assert_eq!(parse_config("gtk_theme = \"Mint-L Darker\"").0.gtk_theme, "Mint-L Darker");
    }

    #[test]
    fn holidays_like_python() {
        assert_eq!(parse_config("holidays = \"BR\"").0.holidays, "BR");
        let (config, warnings) = parse_config("holidays = \"br\"");
        assert_eq!((config.holidays.as_str(), warnings.len()), ("", 1));
        assert_eq!(parse_config("holidays = \"\"").1.len(), 0);
    }

    #[test]
    fn shortcuts_profile() {
        // Os mesmos casos do ShortcutsTests do tests/test_desktop_config.py
        assert_eq!(parse_config("").0.shortcuts, "swaydesk");
        let (config, warnings) = parse_config("shortcuts = \"niri\"");
        assert_eq!((config.shortcuts.as_str(), warnings.len()), ("niri", 0));
        let (config, warnings) = parse_config("shortcuts = \"sway\"");
        assert_eq!(config.shortcuts, "swaydesk");
        assert_eq!(warnings, ["config: invalid shortcuts: sway (use swaydesk, i3, hyprland, niri or dwm)"]);
        assert_eq!(parse_config("shortcuts = 1").1, ["config: invalid value in shortcuts"]);
        assert!(shortcut_keys_file("../etc/passwd").is_none());
    }

    #[test]
    fn power_profiles_in_system() {
        // Os mesmos casos do test_power_profiles_in_system do Python
        assert_eq!(parse_config("").0.power_profiles_in_system, "auto");
        for value in ["auto", "always", "never"] {
            let (config, warnings) = parse_config(&format!("power_profiles_in_system = \"{value}\""));
            assert_eq!((config.power_profiles_in_system.as_str(), warnings.len()), (value, 0));
        }
        let (config, warnings) = parse_config("power_profiles_in_system = \"laptop\"");
        assert_eq!(config.power_profiles_in_system, "auto");
        assert_eq!(warnings, ["config: invalid power_profiles_in_system: laptop (use auto, always or never)"]);
        assert_eq!(parse_config("power_profiles_in_system = true").1, ["config: invalid value in power_profiles_in_system"]);
    }
}
