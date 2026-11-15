//! A config do desktop (`~/.config/sway-desktop/config.toml`), as cores e as fontes: o porte do que
//! os componentes usam de `desktop_config.py`. Um valor ruim vira aviso e não invalida os outros.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_ACCENT: &str = "#199ca8";
/// Abaixo disso, texto preto: todas as cores do Mint-Y ficam com branco, como nos temas.
const MIN_WHITE_CONTRAST: f64 = 2.4;
pub const TEXT_FONT: &str = "Noto Sans";
pub const ICON_FONT: &str = "Symbols Nerd Font";
// As mesmas chaves do Config do desktop_config.py
const KNOWN: &[&str] = &[
    "locale", "weather_location", "holidays", "accent", "gtk_theme", "icon_theme", "folder_color", "file_manager",
    "components",
];

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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            locale: String::new(),
            accent: DEFAULT_ACCENT.to_string(),
            holidays: String::new(),
            weather_location: String::new(),
            gtk_theme: "Mint-Y-Dark-Teal".to_string(),
            icon_theme: "Papirus-Dark".to_string(),
            folder_color: String::new(),
            file_manager: String::new(),
            components: "auto".to_string(),
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
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => return (config, vec![format!("config: could not read: {}", err.message())]),
    };
    for (key, value) in &table {
        if !KNOWN.contains(&key.as_str()) {
            warnings.push(format!("config: unknown option: {key}"));
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
            "file_manager" if !value.is_empty() && !is_name(value, false) => {
                warnings.push(format!("config: invalid file manager: {value} (use the command name, like thunar)"))
            }
            "file_manager" => config.file_manager = value.to_string(),
            _ => {}
        }
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

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
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
}
