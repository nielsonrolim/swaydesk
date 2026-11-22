//! O resto do `desktop_config.py`: onde ficam os temas, os arquivos gerados com a cor de destaque e o
//! `get` dos scripts em Bash. A config e as cores ficam no swaydesk-common.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use swaydesk_common::config::{self, contrast_with_white, data_home, font_css, font_families, mix, palette, Config, Effects, Palette, ICON_FONT};
use swaydesk_common::cursor::{cursor_theme, CURSOR_SIZE, CURSOR_THEMES};

use crate::run_timeout;

pub const FALLBACK_THEME: &str = "Adwaita-dark";
/// Gerenciadores de arquivos conhecidos, na ordem da escolha automática, com o .desktop de cada um
pub const FILE_MANAGERS: &[(&str, &str)] =
    &[("nemo", "nemo"), ("thunar", "thunar"), ("nautilus", "org.gnome.Nautilus"), ("dolphin", "org.kde.dolphin")];
/// Sem o tema de ícones da config instalado, o primeiro destes que estiver
pub const FALLBACK_ICON_THEMES: &[&str] = &["Papirus-Dark", "Papirus", "Mint-Y", "Yaru", "breeze-dark", "breeze", "Adwaita"];
/// O tema de ícones gerado para o Sway: herda o da config e tem as pastas na cor escolhida (papirus)
pub const ICONS_WRAPPER: &str = "sway-desktop-icons";
pub const ICONS_BASE_KEY: &str = "X-SwayDesktop-Base";
pub const GENERATED: &str = "# Gerado pelo sway-desktop-config";
/// Só estes são regravados se já existirem sem o cabeçalho: outro desktop pode cuidar deles.
const SHARED_FILES: &[&str] =
    &["gtk-3.0/settings.ini", "gtk-4.0/settings.ini", "sway-mimeapps.list", "qt5ct/qt5ct.conf", "qt6ct/qt6ct.conf"];
/// Os settings.ini gerados antes do cabeçalho só tinham estas chaves
const OLD_SETTINGS_KEYS: &[&str] = &["gtk-theme-name", "gtk-application-prefer-dark-theme", "gtk-icon-theme-name"];
/// Ordem dos papéis do QPalette nos esquemas do qt5ct e do qt6ct
const QT_ROLES: &[&str] = &[
    "window_text", "button", "light", "midlight", "dark", "mid", "text", "bright_text", "button_text", "base", "window",
    "shadow", "highlight", "highlighted_text", "link", "link_visited", "alternate_base", "no_role", "tooltip_base",
    "tooltip_text", "placeholder_text",
];

pub fn home() -> PathBuf {
    config::home()
}

/// A pasta de config do usuário (a mãe da sway-desktop/config.toml).
pub fn config_home() -> PathBuf {
    config::path().parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| home().join(".config"))
}

/// O share/sway-desktop da instalação, ao lado da pasta do binário (~/.local/bin → ~/.local/share), como
/// o Python usa o da pasta do script. SWAY_DESKTOP_SHARE troca, para os testes rodarem o binário compilado.
pub fn share_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SWAY_DESKTOP_SHARE").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let beside = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.parent()?.join("share/sway-desktop")));
    match beside {
        Some(dir) if dir.is_dir() => dir,
        _ => data_home().join("sway-desktop"),
    }
}

pub fn templates() -> PathBuf {
    share_dir().join("templates")
}

/// Onde ficam os temas de ícones, do que vence para o que perde. SWAY_SYSTEM_ICONS troca o do sistema.
pub fn icon_dirs() -> Vec<PathBuf> {
    let system = std::env::var("SWAY_SYSTEM_ICONS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/share/icons".into());
    vec![home().join(".icons"), data_home().join("icons"), PathBuf::from(system)]
}

/// Onde ficam os temas GTK, do que vence para o que perde.
pub fn theme_dirs() -> Vec<PathBuf> {
    vec![home().join(".themes"), data_home().join("themes"), PathBuf::from("/usr/share/themes")]
}

pub fn theme_installed(name: &str) -> bool {
    theme_dirs().iter().any(|base| base.join(name).join("gtk-3.0/gtk.css").is_file())
}

static DARK_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-[Dd]ark(-|$)|Nokto").unwrap());

/// Pelo nome, para os temas sem cores no CSS. Os Mint-L-Darker são claros.
pub fn is_dark(theme: &str) -> bool {
    DARK_NAME.is_match(theme)
}

pub fn effective_gtk_theme(config: &Config) -> String {
    if theme_installed(&config.gtk_theme) { config.gtk_theme.clone() } else { FALLBACK_THEME.to_string() }
}

/// O valor de `key=` no começo de uma linha, sem os espaços das pontas.
pub fn ini_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('=').map(|v| v.trim().to_string()))
}

/// O tema que o sway-desktop-icons herda primeiro; None se ele não existe.
pub fn icons_wrapper_base() -> Option<String> {
    let text = std::fs::read_to_string(data_home().join("icons").join(ICONS_WRAPPER).join("index.theme")).ok()?;
    ini_value(&text, ICONS_BASE_KEY)
}

pub fn icons_installed(name: &str) -> bool {
    icon_dirs().iter().any(|base| base.join(name).join("index.theme").is_file())
}

/// O tema de ícones da config ou, se ele não estiver instalado, o primeiro de FALLBACK_ICON_THEMES que estiver.
pub fn installed_icon_theme(config: &Config) -> String {
    if icons_installed(&config.icon_theme) {
        return config.icon_theme.clone();
    }
    FALLBACK_ICON_THEMES.iter().find(|n| icons_installed(n)).map_or_else(|| config.icon_theme.clone(), |n| n.to_string())
}

/// O gerado, quando ele é o dos ícones em uso.
pub fn effective_icon_theme(config: &Config) -> String {
    let name = installed_icon_theme(config);
    if icons_wrapper_base().as_deref() == Some(name.as_str()) { ICONS_WRAPPER.to_string() } else { name }
}

/// O da config ou, sem ele, o primeiro instalado. Sem nenhum, o Thunar.
pub fn file_manager(config: &Config) -> String {
    if !config.file_manager.is_empty() {
        return config.file_manager.clone();
    }
    FILE_MANAGERS.iter().map(|(n, _)| *n).find(|n| crate::which(n)).unwrap_or("thunar").to_string()
}

/// Valor para os scripts em Bash; None se a chave não existir.
pub fn get(config: &Config, key: &str) -> Option<String> {
    let env: std::collections::HashMap<String, String> = std::env::vars().collect();
    Some(match key {
        "locale" => config::effective_locale(config, &env),
        "file_manager" => file_manager(config),
        "file_manager_desktop" => {
            let name = file_manager(config);
            FILE_MANAGERS.iter().find(|(n, _)| *n == name).map_or(name.clone(), |(_, d)| d.to_string())
        }
        "weather_location" => config.weather_location.clone(),
        "holidays" => config.holidays.clone(),
        "accent" => config.accent.clone(),
        "gtk_theme" => config.gtk_theme.clone(),
        "icon_theme" => config.icon_theme.clone(),
        "folder_color" => config.folder_color.clone(),
        "components" => config.components.clone(),
        "shortcuts" => config.shortcuts.clone(),
        _ => return None,
    })
}

/// Os modelos que o render_template usa
pub const TEMPLATE_NAMES: &[&str] = &["swaylock.conf", "kitty-theme.conf", "rofi.rasi"];
/// O tema do rofi do sway-launcher, no XDG_DATA_HOME
pub const ROFI_THEME: &str = "sway-desktop/rofi/sway-desktop.rasi";

/// O modelo com as cores, em hexadecimal sem o #. O erro diz o caminho, como o do Python.
pub fn render_template(name: &str, colors: &Palette) -> std::io::Result<String> {
    let path = templates().join(name);
    let mut text = std::fs::read_to_string(&path)
        .map_err(|err| std::io::Error::new(err.kind(), format!("{err}: '{}'", path.display())))?;
    for (key, value) in [("accent", &colors.accent), ("on_accent", &colors.on_accent), ("bright", &colors.bright)] {
        text = text.replace(&format!("{{{{{key}}}}}"), value.trim_start_matches('#'));
    }
    Ok(text)
}

pub fn gtk_settings(config: &Config) -> String {
    format!(
        "{GENERATED}: mude o tema com sway-theme\n[Settings]\ngtk-theme-name={}\ngtk-application-prefer-dark-theme=0\n\
         gtk-icon-theme-name={}\n",
        effective_gtk_theme(config),
        effective_icon_theme(config)
    )
}

/// Um `@define-color nome #rrggbb` do CSS, em minúsculas.
pub fn defined_color(css: &str, name: &str) -> Option<String> {
    let pattern = Regex::new(&format!(r"@define-color {} (#[0-9a-fA-F]{{6}})\b", regex::escape(name))).ok()?;
    pattern.captures(css).map(|c| c[1].to_lowercase())
}

/// Uma cor do CSS do tema GTK, se ela estiver em hexadecimal.
pub fn theme_color(theme: &str, name: &str) -> Option<String> {
    for base in theme_dirs() {
        let Ok(bytes) = std::fs::read(base.join(theme).join("gtk-3.0/gtk.css")) else { continue };
        return defined_color(&String::from_utf8_lossy(&bytes), name);
    }
    None
}

/// A paleta dos apps Qt no Sway, com as cores do tema GTK e o destaque.
pub fn qt_colors(config: &Config, colors: &Palette) -> BTreeMap<&'static str, String> {
    let theme = effective_gtk_theme(config);
    let bg = theme_color(&theme, "theme_bg_color");
    let dark = bg.as_deref().map_or_else(|| is_dark(&theme), |bg| contrast_with_white(bg) > 4.58);
    let bg = bg.unwrap_or_else(|| (if dark { "#2e2e33" } else { "#f6f5f4" }).into());
    let base = theme_color(&theme, "theme_base_color").unwrap_or_else(|| (if dark { "#2e2e33" } else { "#ffffff" }).into());
    let text = theme_color(&theme, "theme_text_color").unwrap_or_else(|| (if dark { "#dadada" } else { "#303030" }).into());
    let shade = if dark { "#ffffff" } else { "#000000" };
    let link = if dark { mix(&colors.accent, "#ffffff", 0.35) } else { mix(&colors.accent, "#000000", 0.2) };
    BTreeMap::from([
        ("window_text", text.clone()),
        ("button", mix(&bg, shade, 0.06)),
        ("light", mix(&bg, "#ffffff", if dark { 0.15 } else { 0.8 })),
        ("midlight", mix(&bg, "#ffffff", if dark { 0.08 } else { 0.4 })),
        ("dark", mix(&bg, "#000000", if dark { 0.3 } else { 0.25 })),
        ("mid", mix(&bg, "#000000", if dark { 0.15 } else { 0.12 })),
        ("text", text.clone()),
        ("bright_text", "#ffffff".into()),
        ("button_text", text.clone()),
        ("base", base.clone()),
        ("window", bg.clone()),
        ("shadow", (if dark { "#141414" } else { "#767676" }).into()),
        ("highlight", colors.accent.clone()),
        ("highlighted_text", colors.on_accent.clone()),
        ("link_visited", mix(&link, "#8a5cc6", 0.5)),
        ("link", link),
        ("alternate_base", mix(&base, shade, 0.04)),
        ("no_role", "#000000".into()),
        ("tooltip_base", if dark { mix(&bg, "#000000", 0.2) } else { "#ffffff".into() }),
        ("tooltip_text", text.clone()),
        ("placeholder_text", mix(&text, &bg, 0.4)),
    ])
}

pub fn qt_color_scheme(config: &Config, colors: &Palette) -> String {
    let palette = qt_colors(config, colors);
    let faded = mix(&palette["text"], &palette["window"], 0.5);
    let mut disabled = palette.clone();
    for role in ["window_text", "text", "button_text", "highlighted_text"] {
        disabled.insert(role, faded.clone());
    }
    let line = |values: &BTreeMap<&str, String>| {
        QT_ROLES.iter().map(|role| format!("#ff{}", values[role].trim_start_matches('#'))).collect::<Vec<_>>().join(", ")
    };
    format!(
        "{GENERATED}: as cores vêm do tema GTK e do destaque\n[ColorScheme]\nactive_colors={}\ndisabled_colors={}\n\
         inactive_colors={}\n",
        line(&palette),
        line(&disabled),
        line(&palette)
    )
}

pub struct Fonts {
    /// Como "Ubuntu 10"
    pub font_name: String,
    pub monospace: String,
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts { font_name: "Sans Serif 10".into(), monospace: "Monospace 10".into() }
    }
}

static FONT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w .+-]+ \d+(\.\d+)?$").unwrap());

/// As fontes do GTK, lidas do dconf. Sem elas, o qt5ct usaria uma monoespaçada.
pub fn gtk_fonts() -> Fonts {
    let mut fonts = Fonts::default();
    for (key, slot) in [("font-name", &mut fonts.font_name), ("monospace-font-name", &mut fonts.monospace)] {
        let out = run_timeout(Command::new("/usr/bin/gsettings").args(["get", "org.gnome.desktop.interface", key]), Duration::from_secs(5));
        let Some((_, stdout)) = out else { continue };
        let value = stdout.trim().trim_matches('\'').to_string();
        if FONT.is_match(&value) {
            *slot = value;
        }
    }
    fonts
}

fn is_number(text: &str) -> bool {
    let (whole, frac) = text.split_once('.').unwrap_or((text, "0"));
    !whole.is_empty() && !frac.is_empty() && whole.bytes().all(|b| b.is_ascii_digit()) && frac.bytes().all(|b| b.is_ascii_digit())
}

/// "Ubuntu 10" -> "Ubuntu": a família de uma fonte no formato do GTK.
pub fn font_family(font: &str) -> String {
    let font = font.trim();
    match font.rsplit_once(' ') {
        Some((family, size)) if is_number(size) => family.to_string(),
        _ => font.to_string(),
    }
}

/// O cursor do seat: vence a linha fixa do config do Sway (o Bibata), que fica para quem não gera os arquivos.
/// Sem nenhum dos temas, nada: o Sway usa o padrão.
pub fn sway_cursor(theme: Option<&str>) -> String {
    match theme {
        Some(name) => format!("{GENERATED}: o cursor instalado\nseat seat0 xcursor_theme {name} {CURSOR_SIZE}\n"),
        None => format!("{GENERATED}: nenhum dos cursores ({}) instalado; fica o padrão\n", CURSOR_THEMES.join(", ")),
    }
}

/// As camadas do layer-shell com o blur do SwayFX (layer_effects), pelo namespace de cada uma: a Waybar, a dock,
/// os popups da Waybar, as notificações, o OSD, o switcher e os painéis de tela inteira (a grade e o menu de
/// sessão), que borram a tela atrás do fundo escurecido. Ficam de fora o popup-click-catcher (a camada de tela
/// inteira, quase transparente, que fecha os popups: borraria a tela toda), o hotspot da dock e a captura (quem
/// escolhe a região ou a janela precisa vê-la nítida). A mesma lista do desktop_config.py.
pub const BLUR_LAYERS: &[&str] = &[
    "waybar",
    "sway-dock",
    "waybar-audio",
    "waybar-battery-popup",
    "waybar-bluetooth",
    "waybar-brightness",
    "waybar-caffeine",
    "waybar-calendar",
    "waybar-clipboard-popup",
    "waybar-media",
    "waybar-network",
    "waybar-notifications-popup",
    "waybar-system",
    "waybar-weather",
    "sway-notifd",
    "sway-osd-bar",
    "sway-switcher",
    "sway-drawer",
    "session-menu",
];

/// Se o `sway` instalado é o SwayFX: a primeira linha do `sway --version` diz "swayfx version ...". O
/// SWAYDESK_SWAYFX (1 ou 0) decide no lugar dele, para os testes.
pub fn is_swayfx() -> bool {
    if let Ok(value) = std::env::var("SWAYDESK_SWAYFX") {
        return value == "1";
    }
    program_is_swayfx("sway")
}

/// Se o programa (um sway) responde "swayfx version ..." ao --version. Sem ele: não.
fn program_is_swayfx(program: impl AsRef<std::ffi::OsStr>) -> bool {
    run_timeout(Command::new(program).arg("--version"), Duration::from_secs(5))
        .is_some_and(|(_, out)| out.lines().next().is_some_and(|line| line.starts_with("swayfx")))
}

/// O config.d/00-keys.conf: os atalhos de todos os perfis (base.conf) e os do perfil do shortcuts, com o caminho
/// inteiro da pasta desta sessão (config::keys_dir). Lido antes do 50-local.conf, que vale mais. O sway-keys o
/// regrava a cada início e reload da sessão.
pub fn sway_keys(config: &Config, keys: &Path) -> String {
    let keys = keys.display();
    format!(
        "# Gerado pelo sway-desktop-config: os atalhos do perfil; mude o shortcuts em ~/.config/sway-desktop/config.toml\n\
         include \"{keys}/base.conf\"\ninclude \"{keys}/{}.conf\"\n",
        config.shortcuts
    )
}

/// O config.d/10-effects.conf. No SwayFX, os efeitos do [effects]; no Sway puro, só um comentário: as diretivas
/// do SwayFX dariam erro de config. O generate roda a cada sessão, então trocar um pelo outro (pacman) vale no
/// próximo login. O animation_duration_ms só entra quando não é 0: o SwayFX 0.5 não conhece a diretiva, e 0 já
/// é o padrão do 0.6.
pub fn sway_effects(effects: &Effects, swayfx: bool) -> String {
    if !swayfx {
        return format!(
            "{GENERATED}: o Sway sem os efeitos; com o SwayFX (no Arch, pelo AUR), o [effects] de\n\
             # ~/.config/sway-desktop/config.toml vale aqui\n"
        );
    }
    let on = |flag: bool| if flag { "enable" } else { "disable" };
    let mut text = format!(
        "{GENERATED}: os efeitos do SwayFX; mude em ~/.config/sway-desktop/config.toml ([effects])\n\
         corner_radius {}\n",
        effects.corner_radius
    );
    text.push_str(&format!("blur {}\n", on(effects.blur)));
    if effects.blur {
        text.push_str("blur_passes 2\nblur_radius 5\n");
    }
    text.push_str(&format!("shadows {}\n", on(effects.shadows)));
    if effects.shadows {
        // Uma sombra leve: preta a 30%
        text.push_str("shadow_blur_radius 20\nshadow_color #0000004D\n");
    }
    text.push_str(&format!("default_dim_inactive {}.{:02}\n", effects.dim_inactive / 100, effects.dim_inactive % 100));
    if effects.animation_duration_ms > 0 {
        text.push_str(&format!("animation_duration_ms {}\n", effects.animation_duration_ms));
    }
    if effects.layer_blur {
        // O blur só onde a camada desenha (blur_ignore_transparent): as margens transparentes dos popups ficam limpas
        for layer in BLUR_LAYERS {
            text.push_str(&format!(
                "layer_effects \"{layer}\" blur enable; blur_ignore_transparent enable; corner_radius {}\n",
                effects.corner_radius
            ));
        }
    }
    text
}

/// A linha font do Sway: a fonte do texto e, se faltar, a do sistema, no tamanho do sistema.
pub fn sway_font(fonts: &Fonts) -> String {
    let (family, size) = fonts.font_name.rsplit_once(' ').unwrap_or(("", &fonts.font_name));
    let families: Vec<String> =
        font_families(Some(family)).into_iter().filter(|n| n != ICON_FONT && n != "sans-serif").collect();
    format!("{GENERATED}: a fonte vem do sistema\nfont pango:{} {size}\n", families.join(", "))
}

/// "Ubuntu 10" no formato do QFont.
pub fn qt_font(gtk_font: &str) -> String {
    let (family, size) = gtk_font.rsplit_once(' ').unwrap_or(("", gtk_font));
    let points = size.parse::<f64>().map(round_half_even).unwrap_or(0);
    format!("\"{family},{points},-1,5,50,0,0,0,0,0\"")
}

fn round_half_even(value: f64) -> i64 {
    let floor = value.floor();
    let base = floor as i64;
    if value - floor > 0.5 || (value - floor == 0.5 && base % 2 != 0) { base + 1 } else { base }
}

pub fn qt_settings(version: u8, config: &Config, fonts: &Fonts) -> String {
    let scheme = config_home().join(format!("qt{version}ct/colors/sway-desktop.conf"));
    format!(
        "{GENERATED}: mude o tema com sway-theme. Editar no qt{version}ct desliga a geração\n[Appearance]\n\
         color_scheme_path={}\ncustom_palette=true\nicon_theme={}\nstandard_dialogs=gtk3\nstyle=Fusion\n\n\
         [Fonts]\ngeneral={}\nfixed={}\n",
        scheme.display(),
        effective_icon_theme(config),
        qt_font(&fonts.font_name),
        qt_font(&fonts.monospace)
    )
}

/// Pastas abertas por outros apps no Sway vão para o gerenciador da config.
pub fn mimeapps(config: &Config) -> String {
    let desktop = get(config, "file_manager_desktop").unwrap_or_default();
    format!(
        "{GENERATED}: mude o gerenciador em ~/.config/sway-desktop/config.toml (file_manager)\n\
         [Default Applications]\ninode/directory={desktop}.desktop\n"
    )
}

/// A folha da Waybar que o pacote swaydesk-desktop instala, sem os @import (o embrulho abaixo os faz)
pub const PACKAGE_WAYBAR_STYLE: &str = "/usr/share/swaydesk-desktop/waybar/style.css";

/// A folha da Waybar na sessão do pacote: as cores e as fontes geradas, depois a do pacote. Os caminhos das
/// duas primeiras são relativos a este arquivo (~/.config/swaydesk-desktop/waybar/style.css).
/// SWAYDESK_WAYBAR_STYLE troca a do pacote, para os testes.
pub fn waybar_wrapper() -> String {
    let base = std::env::var("SWAYDESK_WAYBAR_STYLE").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| PACKAGE_WAYBAR_STYLE.into());
    let base = base.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "/* Gerado pelo sway-desktop-config: a Waybar da sessão do pacote (swaydesk-desktop) */\n\
         @import url(\"../../waybar/accent.css\");\n\
         @import url(\"../../waybar/fonts.css\");\n\
         @import url(\"{base}\");\n"
    )
}

/// O que depende do tema e não pode lê-lo: {caminho relativo à pasta de config: conteúdo}, na ordem do Python.
pub fn generated_files(colors: &Palette, config: &Config, fonts: &Fonts) -> std::io::Result<Vec<(&'static str, String)>> {
    let css = config::css_colors(colors);
    let sway = format!(
        "# Gerado pelo sway-desktop-config: mude a cor em ~/.config/sway-desktop/config.toml\n\
         client.focused {a} {a} {} {} {a}\n",
        colors.on_accent,
        colors.bright,
        a = colors.accent
    );
    let settings = gtk_settings(config);
    let scheme = qt_color_scheme(config, colors);
    let fonts_css = format!("/* Gerado pelo sway-desktop-config: a fonte do sistema */\n{}", font_css(Some(&font_family(&fonts.font_name))));
    Ok(vec![
        ("waybar/accent.css", css),
        ("sway/config.d/00-accent.conf", sway),
        ("sway/config.d/00-font.conf", sway_font(fonts)),
        ("sway/config.d/00-cursor.conf", sway_cursor(cursor_theme())),
        ("sway/config.d/00-keys.conf", sway_keys(config, &swaydesk_common::config::keys_dir())),
        ("sway/config.d/10-effects.conf", sway_effects(&config.effects, is_swayfx())),
        ("waybar/fonts.css", fonts_css),
        ("swaylock/config", render_template("swaylock.conf", colors)?),
        ("gtk-3.0/settings.ini", settings.clone()),
        ("gtk-4.0/settings.ini", settings),
        ("sway-mimeapps.list", mimeapps(config)),
        ("qt5ct/qt5ct.conf", qt_settings(5, config, fonts)),
        ("qt6ct/qt6ct.conf", qt_settings(6, config, fonts)),
        ("qt5ct/colors/sway-desktop.conf", scheme.clone()),
        ("qt6ct/colors/sway-desktop.conf", scheme),
        // Sempre, mesmo no modo dotfiles: a sessão do pacote pode ser a próxima a abrir
        ("swaydesk-desktop/waybar/style.css", waybar_wrapper()),
    ])
}

/// Troca o caminho de uma vez, sem escrever através de um link para o repositório.
pub fn write_file(file: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = file.with_file_name(format!("{name}.tmp.{}", std::process::id()));
    let result = std::fs::write(&tmp, content).and_then(|_| std::fs::rename(&tmp, file));
    let _ = std::fs::remove_file(&tmp);
    result
}

/// O arquivo do usuário, ou o destino dele se for um link (`file.resolve() if file.is_symlink()`).
pub fn resolved(file: &Path) -> PathBuf {
    if file.is_symlink() { file.canonicalize().unwrap_or_else(|_| file.to_path_buf()) } else { file.to_path_buf() }
}

/// Grava os arquivos. Devolve os compartilhados que ficaram como estavam, por serem de outro.
pub fn generate(config: &Config) -> std::io::Result<Vec<String>> {
    generate_with(config, &gtk_fonts())
}

pub fn generate_with(config: &Config, fonts: &Fonts) -> std::io::Result<Vec<String>> {
    let colors = palette(config);
    let base = config_home();
    let mut kept = Vec::new();
    for (name, content) in generated_files(&colors, config, fonts)? {
        if SHARED_FILES.contains(&name) && !ours(&base.join(name)) {
            kept.push(name.to_string());
            continue;
        }
        write_file(&base.join(name), &content)?;
    }
    // O tema do rofi do sway-launcher (Alt+Espaço)
    write_file(&data_home().join(ROFI_THEME), &render_template("rofi.rasi", &colors)?)?;
    Ok(kept)
}

/// O arquivo não existe ou foi gerado por aqui, inclusive um settings.ini de antes do cabeçalho. Um link também.
pub fn ours(file: &Path) -> bool {
    if file.is_symlink() {
        return true;
    }
    let text = match std::fs::read(file) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return true,
        Err(_) => return false,
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return false,
        },
    };
    if text.starts_with(GENERATED) {
        return true;
    }
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    file.file_name().is_some_and(|n| n == "settings.ini")
        && lines.first() == Some(&"[Settings]")
        && lines[1..].iter().all(|l| OLD_SETTINGS_KEYS.contains(&l.split('=').next().unwrap_or("")))
}

/// O papel de parede que o config do Sway mostra (`output * bg ~/.local/share/wallpaper/current.jpg`).
const WALLPAPER: &str = ".local/share/wallpaper/current.jpg";

/// As imagens lisas de onde o papel volta: a do skel do pacote e, nos dotfiles, a do home-copy/ do repositório
/// (achado pelo link ~/.config/sway/config, que aponta para home/.config/sway/config).
fn wallpaper_seeds() -> Vec<PathBuf> {
    let mut seeds = vec![crate::setup::skel_dir().join(WALLPAPER)];
    let sway = config_home().join("sway/config");
    if let Some(repo) = std::fs::canonicalize(sway).ok().and_then(|p| p.ancestors().nth(4).map(Path::to_path_buf)) {
        seeds.push(repo.join("home-copy").join(WALLPAPER));
    }
    seeds
}

/// Sem o papel (o arquivo não existe, ou é um link quebrado), copia o primeiro das sementes que existir; um papel
/// em uso nunca é tocado. Devolve se o papel existe no fim. O ensure_wallpaper do desktop_config.py.
fn ensure_wallpaper_at(target: &Path, seeds: &[PathBuf]) -> bool {
    if target.exists() {
        return true;
    }
    let Some(seed) = seeds.iter().find(|p| p.is_file()) else { return false };
    if target.is_symlink() {
        let _ = std::fs::remove_file(target);
    }
    target.parent().is_some_and(|dir| std::fs::create_dir_all(dir).is_ok()) && std::fs::copy(seed, target).is_ok()
}

pub fn ensure_wallpaper() -> bool {
    ensure_wallpaper_at(&home().join(WALLPAPER), &wallpaper_seeds())
}

/// Devolve se o Sway recarregou. Sem sessão do Sway, não há o que recarregar. Antes, sem o papel de parede, a
/// imagem lisa volta: senão o `output * bg` do config faz cada reload abrir o swaynag ("unable to access
/// background file").
pub fn reload_sway() -> bool {
    ensure_wallpaper();
    run_timeout(Command::new("swaymsg").arg("reload").stdin(Stdio::null()), Duration::from_secs(5)).is_some_and(|(code, _)| code == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallpaper_comes_back_only_when_missing() {
        let dir = std::env::temp_dir().join(format!("swaydesk-wallseed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let seed = dir.join("seed.jpg");
        std::fs::write(&seed, b"liso").unwrap();
        let seeds = [dir.join("sem-skel.jpg"), seed.clone()];
        let target = dir.join("home/.local/share/wallpaper/current.jpg");
        // Ausente: a pasta e a cópia
        assert!(ensure_wallpaper_at(&target, &seeds));
        assert_eq!(std::fs::read(&target).unwrap(), b"liso");
        // Um papel em uso, arquivo ou link, fica
        std::fs::write(&target, b"meu").unwrap();
        assert!(ensure_wallpaper_at(&target, &seeds));
        assert_eq!(std::fs::read(&target).unwrap(), b"meu");
        let mine = dir.join("bing.jpg");
        std::fs::write(&mine, b"bing").unwrap();
        std::fs::remove_file(&target).unwrap();
        std::os::unix::fs::symlink(&mine, &target).unwrap();
        assert!(ensure_wallpaper_at(&target, &seeds));
        assert!(target.is_symlink());
        // O link quebrado sai e a imagem lisa entra no lugar
        std::fs::remove_file(&mine).unwrap();
        assert!(ensure_wallpaper_at(&target, &seeds));
        assert!(!target.is_symlink());
        assert_eq!(std::fs::read(&target).unwrap(), b"liso");
        // Sem semente: nada muda
        std::fs::remove_file(&target).unwrap();
        assert!(!ensure_wallpaper_at(&target, &[dir.join("nada.jpg")]));
        assert!(!target.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sway_keys_like_python() {
        // O mesmo texto do ShortcutsTests do tests/test_desktop_config.py
        let config = Config { shortcuts: "dwm".into(), ..Config::default() };
        assert_eq!(
            sway_keys(&config, Path::new("/k")),
            "# Gerado pelo sway-desktop-config: os atalhos do perfil; mude o shortcuts em ~/.config/sway-desktop/config.toml\n\
             include \"/k/base.conf\"\ninclude \"/k/dwm.conf\"\n"
        );
        assert!(sway_keys(&Config::default(), Path::new("/k")).ends_with("include \"/k/swaydesk.conf\"\n"));
    }

    #[test]
    fn sway_effects_like_python() {
        // Os mesmos textos do EffectsTests do tests/test_desktop_config.py
        let head = "# Gerado pelo sway-desktop-config: os efeitos do SwayFX; mude em ~/.config/sway-desktop/config.toml ([effects])\n";
        let default = format!(
            "{head}corner_radius 10\nblur enable\nblur_passes 2\nblur_radius 5\nshadows enable\nshadow_blur_radius 20\n\
             shadow_color #0000004D\ndefault_dim_inactive 0.00\n"
        );
        let layers: String = BLUR_LAYERS
            .iter()
            .map(|l| format!("layer_effects \"{l}\" blur enable; blur_ignore_transparent enable; corner_radius 10\n"))
            .collect();
        assert_eq!(sway_effects(&Effects::default(), true), default);
        assert_eq!(sway_effects(&Effects { layer_blur: true, ..Effects::default() }, true), format!("{default}{layers}"));
        let off = Effects { corner_radius: 0, blur: false, shadows: false, layer_blur: false, dim_inactive: 15, animation_duration_ms: 0 };
        assert_eq!(sway_effects(&off, true), format!("{head}corner_radius 0\nblur disable\nshadows disable\ndefault_dim_inactive 0.15\n"));
        let animated = Effects { animation_duration_ms: 250, ..Effects::default() };
        assert_eq!(sway_effects(&animated, true), format!("{default}animation_duration_ms 250\n"));
        assert!(!sway_effects(&animated, false).contains("animation"));
        let plain = sway_effects(&Effects::default(), false);
        assert_eq!(
            plain,
            "# Gerado pelo sway-desktop-config: o Sway sem os efeitos; com o SwayFX (no Arch, pelo AUR), o [effects] de\n\
             # ~/.config/sway-desktop/config.toml vale aqui\n"
        );
        assert!(plain.lines().all(|l| l.starts_with('#')));
        assert!(!BLUR_LAYERS.contains(&"popup-click-catcher") && !BLUR_LAYERS.contains(&"sway-capture"));
    }

    #[test]
    fn swayfx_detection_by_version() {
        // Um sway de mentira, que responde como o SwayFX ou como o Sway
        let dir = std::env::temp_dir().join(format!("swaydesk-swayfx-detection-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, line, want) in [("fx", "swayfx version 0.6.1", true), ("plain", "sway version 1.12", false)] {
            let sway = dir.join(name);
            std::fs::write(&sway, format!("#!/bin/sh\necho '{line}'\n")).unwrap();
            std::fs::set_permissions(&sway, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            assert_eq!(program_is_swayfx(&sway), want, "{line}");
        }
        assert!(!program_is_swayfx(dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sway_cursor_like_python() {
        // O mesmo texto do sway_cursor do desktop_config.py
        assert_eq!(sway_cursor(Some("capitaine-cursors")),
                   "# Gerado pelo sway-desktop-config: o cursor instalado\nseat seat0 xcursor_theme capitaine-cursors 24\n");
        assert_eq!(sway_cursor(None),
                   "# Gerado pelo sway-desktop-config: nenhum dos cursores (Bibata-Modern-Classic, capitaine-cursors) instalado; fica o padrão\n");
    }

    #[test]
    fn font_helpers_like_python() {
        assert_eq!(font_family("Ubuntu 10"), "Ubuntu");
        assert_eq!(font_family("Noto Sans 10.5"), "Noto Sans");
        assert_eq!(font_family("Sans"), "Sans");
        assert_eq!(qt_font("Ubuntu 10.5"), "\"Ubuntu,10,-1,5,50,0,0,0,0,0\"");
        assert_eq!(qt_font("Ubuntu 11.5"), "\"Ubuntu,12,-1,5,50,0,0,0,0,0\"");
        let fonts = Fonts { font_name: "Ubuntu 11".into(), monospace: "Mono 10".into() };
        assert_eq!(sway_font(&fonts), format!("{GENERATED}: a fonte vem do sistema\nfont pango:Noto Sans, Ubuntu 11\n"));
    }

    #[test]
    fn dark_names() {
        for (theme, dark) in [("Mint-Y-Dark-Red", true), ("Mint-L-Darker", false), ("Adwaita-dark", true), ("Adapta-Nokto", true), ("Adwaita", false)] {
            assert_eq!(is_dark(theme), dark, "{theme}");
        }
    }

    #[test]
    fn colors_from_css() {
        let css = "@define-color theme_bg_color #ABCDEF;\n@define-color accent_color #123456x;";
        assert_eq!(defined_color(css, "theme_bg_color").as_deref(), Some("#abcdef"));
        assert_eq!(defined_color(css, "accent_color"), None);
    }
}
