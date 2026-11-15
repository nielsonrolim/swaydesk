"""A config única do desktop (~/.config/sway-desktop/config.toml), sem GTK.

Arquivo ausente ou inválido usa os padrões: nenhum widget pode deixar de abrir por causa dele.
"""
import os
import re
import shutil
import subprocess
import sys
import tomllib
from dataclasses import dataclass, fields, replace
from pathlib import Path

LOCALE = re.compile(r"[a-z]{2}_[A-Z]{2}")
HOLIDAYS = re.compile(r"([A-Z]{2}|none)?")  # o país dos feriados (python3-holidays); vazio: o do locale;
# "none": nenhum
ACCENT = re.compile(r"#[0-9a-fA-F]{6}")
DEFAULT_ACCENT = "#199ca8"
MIN_WHITE_CONTRAST = 2.4  # abaixo disso, texto preto: todas as cores do Mint-Y ficam com branco, como nos temas
TEMPLATES = Path(__file__).resolve().parents[1] / "share/sway-desktop/templates"
TEMPLATE_NAMES = ("swaylock.conf", "kitty-theme.conf", "rofi.rasi")  # os que o render_template usa
THEME_NAME = re.compile(r"[\w .+-]+")
FALLBACK_THEME = "Adwaita-dark"  # vem com o GTK: é o que existe quando o tema pedido não está instalado
# Gerenciadores de arquivos conhecidos, na ordem da escolha automática, com o .desktop de cada um
# (para a dock). O Nemo vem com o Mint; no Ubuntu e no Debian, o --deps instala o Thunar, mais leve
FILE_MANAGERS = {"nemo": "nemo", "thunar": "thunar", "nautilus": "org.gnome.Nautilus", "dolphin": "org.kde.dolphin"}
COMMAND_NAME = re.compile(r"[\w.+-]+")
# As cores do papirus-folders, com a cor da frente da pasta (48x48/places/folder-<cor>.svg). A adwaita e
# a yaru são o destaque padrão desses temas, e não a cor da pasta
PAPIRUS_COLORS = {
    "adwaita": "#3584e4", "black": "#4f4f4f", "blue": "#5294e2", "bluegrey": "#607d8b", "breeze": "#57b8ec",
    "brown": "#ae8e6c", "carmine": "#a30002", "cyan": "#00bcd4", "darkcyan": "#45abb7", "deeporange": "#eb6637",
    "green": "#87b158", "grey": "#8e8e8e", "indigo": "#5c6bc0", "magenta": "#ca71df", "nordic": "#81a1c1",
    "orange": "#ee923a", "palebrown": "#d1bfae", "paleorange": "#eeca8f", "pink": "#f06292", "red": "#e25252",
    "teal": "#16a085", "violet": "#7e57c2", "white": "#e4e4e4", "yaru": "#e95420", "yellow": "#f9bd30",
}
# Sem o tema de ícones da config instalado (o Papirus sem o --deps), o primeiro destes que estiver
FALLBACK_ICON_THEMES = ("Papirus-Dark", "Papirus", "Mint-Y", "Yaru", "breeze-dark", "breeze", "Adwaita")
# O tema de ícones gerado para o Sway: herda o da config e tem as pastas na cor escolhida (sway_papirus)
ICONS_WRAPPER = "sway-desktop-icons"
ICONS_BASE_KEY = "X-SwayDesktop-Base"


@dataclass(frozen=True)
class Config:
    locale: str = ""  # como "pt_BR"; vazio: o do ambiente
    weather_location: str = ""  # vazio: vem do IP
    holidays: str = ""  # o código do país, como "BR"; vazio: o do locale; "none": nenhum
    accent: str = DEFAULT_ACCENT  # como "#199ca8"
    gtk_theme: str = "Mint-Y-Dark-Teal"
    icon_theme: str = "Papirus-Dark"
    folder_color: str = ""  # a cor das pastas do Papirus, como "teal"; vazio: a mais perto do destaque
    file_manager: str = ""  # comando, como "thunar"; vazio: o primeiro instalado de FILE_MANAGERS
    components: str = "auto"  # a versão dos componentes: auto (Rust onde houver), python ou rust


@dataclass(frozen=True)
class Palette:
    accent: str
    hover: str  # o destaque sob o mouse
    bright: str  # o indicador de onde a próxima janela abre, no Sway
    soft: str  # texto secundário sobre o destaque
    on_accent: str  # texto sobre o destaque


def config_home():
    return Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")


def path():
    return config_home() / "sway-desktop" / "config.toml"


COMPONENTS = ("auto", "python", "rust")


def parse_config(data):
    """Devolve a config e os avisos: um valor ruim não invalida os outros."""
    config, warnings = Config(), []
    known = {f.name for f in fields(Config)}
    for key, value in data.items():
        if key not in known:
            warnings.append(f"config: unknown option: {key}")
        elif not isinstance(value, str):
            warnings.append(f"config: invalid value in {key}")
        elif key == "locale" and value and not LOCALE.fullmatch(value):
            warnings.append(f"config: invalid locale: {value} (use one like pt_BR)")
        elif key == "holidays" and not HOLIDAYS.fullmatch(value):
            warnings.append(f"config: invalid holidays: {value} (use a country code like BR, none, or empty)")
        elif key == "accent" and not ACCENT.fullmatch(value):
            warnings.append(f"config: invalid accent color: {value} (use one like {DEFAULT_ACCENT})")
        elif key == "accent":
            config = replace(config, accent=value.lower())
        elif key in ("gtk_theme", "icon_theme") and not THEME_NAME.fullmatch(value):
            warnings.append(f"config: invalid theme name in {key}: {value}")
        elif key == "folder_color" and value and value not in PAPIRUS_COLORS:
            warnings.append(f"config: invalid folder color: {value} (use one of sway-theme colors, or empty)")
        elif key == "components" and value not in COMPONENTS:
            warnings.append(f"config: invalid components: {value} (use auto, python or rust)")
        elif key == "file_manager" and value and not COMMAND_NAME.fullmatch(value):
            warnings.append(f"config: invalid file manager: {value} (use the command name, like thunar)")
        else:
            config = replace(config, **{key: value})
    return config, warnings


def load_config(file):
    try:
        with open(file, "rb") as f:
            return parse_config(tomllib.load(f))
    except FileNotFoundError:
        return Config(), []
    except (OSError, tomllib.TOMLDecodeError, UnicodeDecodeError) as err:
        return Config(), [f"config: could not read {file}: {err}"]


def load():
    config, warnings = load_config(path())
    for warning in warnings:
        print(warning, file=sys.stderr)
    return config


def effective_locale(config, env=os.environ):
    """O locale da config ou, sem ele, o de LC_ALL, LC_TIME ou LANG, nessa ordem. C e POSIX: vazio."""
    if config.locale:
        return config.locale
    for name in ("LC_ALL", "LC_TIME", "LANG"):
        if env.get(name):
            m = LOCALE.match(env[name])
            return m.group() if m else ""
    return ""


def message_languages(config, env=os.environ):
    """As línguas dos textos, em ordem, para o gettext: a do config ou, sem ela, a do ambiente
    (LANGUAGE, LC_ALL, LC_MESSAGES, LANG). C, POSIX ou nada: lista vazia, o inglês do código."""
    if config.locale:
        return [config.locale]
    if env.get("LANGUAGE"):
        return [name for name in env["LANGUAGE"].split(":") if name]
    for name in ("LC_ALL", "LC_MESSAGES", "LANG"):
        if env.get(name):
            value = env[name].partition(".")[0].partition("@")[0]
            return [] if value in ("C", "POSIX") else [value]
    return []


def holidays_country(config, env=os.environ):
    """O país dos feriados e se ele veio da config: (país, explícito). Sem o holidays na config, o país do
    locale ("BR" em pt_BR), para os feriados aparecerem sem configurar nada; "none" desliga. C, POSIX ou
    nada: nenhum."""
    if config.holidays == "none":
        return "", True
    if config.holidays:
        return config.holidays, True
    return effective_locale(config, env).partition("_")[2], False


def language(config, env=os.environ):
    """A parte do idioma: "pt" em "pt_BR"."""
    return effective_locale(config, env).partition("_")[0]


def file_manager(config, which=shutil.which):
    """O da config ou, sem ele, o primeiro instalado. Sem nenhum, o Thunar, que é o que o --deps instala."""
    if config.file_manager:
        return config.file_manager
    return next((name for name in FILE_MANAGERS if which(name)), "thunar")


def get(config, key, env=os.environ):
    """Valor para os scripts em Bash; None se a chave não existir."""
    if key == "locale":
        return effective_locale(config, env)
    if key == "file_manager":
        return file_manager(config)
    if key == "file_manager_desktop":  # o .desktop, para fixar na dock
        name = file_manager(config)
        return FILE_MANAGERS.get(name, name)
    if key in ("weather_location", "holidays", "accent", "gtk_theme", "icon_theme", "folder_color", "components"):
        return getattr(config, key)
    return None


def channels(color):
    """ "#ff8000" -> [255, 128, 0]"""
    return [int(color[i:i + 2], 16) for i in (1, 3, 5)]


def linear_rgb(color):
    """Os canais sem a curva do sRGB, de 0 a 1: a base do contraste e do Lab."""
    def linear(channel):
        value = channel / 255
        return value / 12.92 if value <= 0.04045 else ((value + 0.055) / 1.055) ** 2.4
    return [linear(c) for c in channels(color)]


def mix(color, other, amount):
    """A cor com uma parte (de 0 a 1) da outra."""
    return "#" + "".join(f"{round(a + (b - a) * amount):02x}" for a, b in zip(channels(color), channels(other)))


def contrast_with_white(color):
    """Contraste do WCAG com o branco, de 1 a 21."""
    r, g, b = linear_rgb(color)
    return 1.05 / (0.2126 * r + 0.7152 * g + 0.0722 * b + 0.05)


def palette(config):
    """Os tons do destaque. O padrão usa os tons do tema Mint-Y; as outras cores, tons calculados."""
    accent = config.accent
    if accent == DEFAULT_ACCENT:
        return Palette(accent, "#22b3c0", "#29d0df", "#e0f4f6", "#ffffff")
    on_accent = "#ffffff" if contrast_with_white(accent) >= MIN_WHITE_CONTRAST else "#000000"
    hover = mix(accent, "#ffffff" if on_accent == "#ffffff" else "#000000", 0.12)
    return Palette(accent, hover, mix(accent, "#ffffff", 0.25), mix(accent, on_accent, 0.85), on_accent)


def css_colors(colors):
    """As cores com nome do CSS do GTK. busy_sub não depende do destaque: fica sobre fundo cinza."""
    return (f"@define-color accent {colors.accent};\n"
            f"@define-color accent_hover {colors.hover};\n"
            f"@define-color accent_soft {colors.soft};\n"
            f"@define-color on_accent {colors.on_accent};\n"
            "@define-color busy_sub #e0f4f6;\n")


def render_template(name, colors):
    """O modelo com as cores, em hexadecimal sem o #: cada programa pede um formato."""
    text = (TEMPLATES / name).read_text(encoding="utf-8")
    for key, value in (("accent", colors.accent), ("on_accent", colors.on_accent), ("bright", colors.bright)):
        text = text.replace("{{" + key + "}}", value.removeprefix("#"))
    return text


def data_home():
    return Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")


def cache_home():
    return Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")


def icon_dirs():
    """Onde ficam os temas de ícones, do que vence para o que perde. SWAY_SYSTEM_ICONS troca o do
    sistema: os testes não dependem do que está instalado em quem os roda."""
    system = os.environ.get("SWAY_SYSTEM_ICONS") or "/usr/share/icons"
    return [Path.home() / ".icons", data_home() / "icons", Path(system)]


def theme_dirs():
    """Onde ficam os temas GTK, do que vence para o que perde."""
    return [Path.home() / ".themes", data_home() / "themes", Path("/usr/share/themes")]


def theme_installed(name):
    return any((base / name / "gtk-3.0" / "gtk.css").is_file() for base in theme_dirs())


def is_dark(theme):
    """Pelo nome, para os temas sem cores no CSS. Os Mint-L-Darker são claros: só a barra de título é escura."""
    return re.search(r"-[Dd]ark(-|$)|Nokto", theme) is not None


def effective_gtk_theme(config):
    return config.gtk_theme if theme_installed(config.gtk_theme) else FALLBACK_THEME


def icons_wrapper_base():
    """O tema que o sway-desktop-icons herda primeiro; None se ele não existe."""
    try:
        text = (data_home() / "icons" / ICONS_WRAPPER / "index.theme").read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return None
    match = re.search(rf"^{ICONS_BASE_KEY}=(.*)$", text, flags=re.M)
    return match.group(1).strip() if match else None


def icons_installed(name):
    return any((base / name / "index.theme").is_file() for base in icon_dirs())


def installed_icon_theme(config):
    """O tema de ícones da config ou, se ele não estiver instalado, o primeiro de FALLBACK_ICON_THEMES que
    estiver. Assim um padrão Papirus sem o pacote não deixa os apps só com os ícones do hicolor."""
    if icons_installed(config.icon_theme):
        return config.icon_theme
    return next((name for name in FALLBACK_ICON_THEMES if icons_installed(name)), config.icon_theme)


def effective_icon_theme(config):
    """O gerado, quando ele é o dos ícones em uso: tem as pastas na cor e o Breeze Dark de reserva."""
    name = installed_icon_theme(config)
    return ICONS_WRAPPER if icons_wrapper_base() == name else name


def gtk_settings(config):
    # prefer-dark sempre 0: os temas escuros já são escuros. O GTK só lê esse valor ao iniciar, e com 1
    # uma janela aberta seguiria na variante escura depois da troca para um tema claro
    return (f"{GENERATED}: mude o tema com sway-theme\n"
            "[Settings]\n"
            f"gtk-theme-name={effective_gtk_theme(config)}\n"
            "gtk-application-prefer-dark-theme=0\n"
            f"gtk-icon-theme-name={effective_icon_theme(config)}\n")


# Ordem dos papéis do QPalette nos esquemas do qt5ct e do qt6ct
QT_ROLES = ("window_text", "button", "light", "midlight", "dark", "mid", "text", "bright_text", "button_text", "base",
            "window", "shadow", "highlight", "highlighted_text", "link", "link_visited", "alternate_base", "no_role",
            "tooltip_base", "tooltip_text", "placeholder_text")
QT_FONTS = {"font-name": "Sans Serif 10", "monospace-font-name": "Monospace 10"}


def theme_color(theme, name):
    """Uma cor do CSS do tema GTK, se ela estiver em hexadecimal."""
    for base in theme_dirs():
        try:
            css = (base / theme / "gtk-3.0" / "gtk.css").read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        match = re.search(rf"@define-color {name} (#[0-9a-fA-F]{{6}})\b", css)
        return match.group(1).lower() if match else None
    return None


def qt_colors(config, colors):
    """A paleta dos apps Qt no Sway, com as cores do tema GTK e o destaque. O plugin gtk3 do Qt 5, e
    do Qt 6 até o 6.4, não passa a paleta: sem isso, o VLC e os outros apps Qt ficavam claros."""
    theme = effective_gtk_theme(config)
    bg = theme_color(theme, "theme_bg_color")
    dark = contrast_with_white(bg) > 4.58 if bg else is_dark(theme)
    bg = bg or ("#2e2e33" if dark else "#f6f5f4")
    base = theme_color(theme, "theme_base_color") or ("#2e2e33" if dark else "#ffffff")
    text = theme_color(theme, "theme_text_color") or ("#dadada" if dark else "#303030")
    shade = "#ffffff" if dark else "#000000"
    link = mix(colors.accent, "#ffffff", 0.35) if dark else mix(colors.accent, "#000000", 0.2)
    return {
        "window_text": text, "button": mix(bg, shade, 0.06), "light": mix(bg, "#ffffff", 0.15 if dark else 0.8),
        "midlight": mix(bg, "#ffffff", 0.08 if dark else 0.4), "dark": mix(bg, "#000000", 0.3 if dark else 0.25),
        "mid": mix(bg, "#000000", 0.15 if dark else 0.12), "text": text, "bright_text": "#ffffff",
        "button_text": text, "base": base, "window": bg, "shadow": "#141414" if dark else "#767676",
        "highlight": colors.accent, "highlighted_text": colors.on_accent, "link": link,
        "link_visited": mix(link, "#8a5cc6", 0.5), "alternate_base": mix(base, shade, 0.04), "no_role": "#000000",
        "tooltip_base": mix(bg, "#000000", 0.2) if dark else "#ffffff", "tooltip_text": text,
        "placeholder_text": mix(text, bg, 0.4),
    }


def qt_color_scheme(config, colors):
    palette_ = qt_colors(config, colors)
    faded = mix(palette_["text"], palette_["window"], 0.5)
    disabled = {**palette_, **{role: faded for role in ("window_text", "text", "button_text", "highlighted_text")}}

    def line(values):
        return ", ".join("#ff" + values[role].removeprefix("#") for role in QT_ROLES)
    return (f"{GENERATED}: as cores vêm do tema GTK e do destaque\n[ColorScheme]\n"
            f"active_colors={line(palette_)}\ndisabled_colors={line(disabled)}\ninactive_colors={line(palette_)}\n")


def gtk_fonts():
    """As fontes do GTK, como "Ubuntu 10", lidas do dconf. Sem elas, o qt5ct usaria uma monoespaçada."""
    fonts = dict(QT_FONTS)
    for key in fonts:
        try:
            out = subprocess.run(["/usr/bin/gsettings", "get", "org.gnome.desktop.interface", key],
                                 capture_output=True, text=True, timeout=5, check=False).stdout.strip().strip("'")
        except (OSError, subprocess.SubprocessError):
            continue
        if re.fullmatch(r"[\w .+-]+ \d+(\.\d+)?", out):
            fonts[key] = out
    return fonts


# A fonte do texto, empacotada em todas as distros suportadas (fonts-noto-core, noto-fonts no Arch),
# e a dos ícones, só com símbolos: não tem letras, então nunca toma o lugar do texto. Sem a do texto,
# vale a fonte do sistema
TEXT_FONT = "Noto Sans"
ICON_FONT = "Symbols Nerd Font"


def font_family(font):
    """ "Ubuntu 10" -> "Ubuntu": a família de uma fonte no formato do GTK."""
    family, _, size = (font or "").strip().rpartition(" ")
    return family if re.fullmatch(r"\d+(\.\d+)?", size) else (font or "").strip()


def font_families(system=None):
    """As famílias, em ordem: a do texto, a do sistema, a dos ícones e a genérica."""
    families = []
    for family in (TEXT_FONT, system, ICON_FONT, "sans-serif"):
        if family and family not in families:
            families.append(family)
    return families


def font_css(system=None):
    """A regra que dá as fontes a tudo. Vem antes do resto: as classes de ícones a sobrepõem."""
    names = ", ".join(name if name == "sans-serif" else f'"{name}"' for name in font_families(system))
    return f"* {{ font-family: {names}; }}\n"


# O tema do cursor, na ordem de preferência: o Bibata, que o Debian, o Ubuntu e o Mint empacotam; onde ele não
# existe (o Arch só o tem no AUR), o Capitaine, o mais parecido nos repositórios oficiais. O mesmo do
# swaydesk_common::cursor
CURSOR_THEMES = ("Bibata-Modern-Classic", "capitaine-cursors")
CURSOR_SIZE = 24


def cursor_theme():
    """O primeiro de CURSOR_THEMES com cursores (a pasta cursors/) numa das pastas de ícones, ou None."""
    return next((name for name in CURSOR_THEMES if any((base / name / "cursors").is_dir() for base in icon_dirs())),
                None)


def sway_cursor(theme):
    """O cursor do seat: vence a linha fixa do config do Sway (o Bibata). Sem nenhum, o padrão do Sway."""
    if theme:
        return f"{GENERATED}: o cursor instalado\nseat seat0 xcursor_theme {theme} {CURSOR_SIZE}\n"
    return f"{GENERATED}: nenhum dos cursores ({', '.join(CURSOR_THEMES)}) instalado; fica o padrão\n"


def sway_font(fonts):
    """A linha font do Sway: a fonte do texto e, se faltar, a do sistema, no tamanho do sistema."""
    family, _, size = fonts["font-name"].rpartition(" ")
    families = [name for name in font_families(family) if name not in (ICON_FONT, "sans-serif")]
    return f"{GENERATED}: a fonte vem do sistema\nfont pango:{', '.join(families)} {size}\n"


def qt_font(gtk_font):
    """ "Ubuntu 10" no formato do QFont: família, tamanho em pontos e os padrões."""
    family, _, size = gtk_font.rpartition(" ")
    return f'"{family},{round(float(size))},-1,5,50,0,0,0,0,0"'


def qt_settings(version, config, fonts):
    scheme = path().parents[1] / f"qt{version}ct/colors/sway-desktop.conf"
    return (f"{GENERATED}: mude o tema com sway-theme. Editar no qt{version}ct desliga a geração\n"
            f"[Appearance]\ncolor_scheme_path={scheme}\ncustom_palette=true\nicon_theme={effective_icon_theme(config)}\n"
            "standard_dialogs=gtk3\nstyle=Fusion\n\n"
            f"[Fonts]\ngeneral={qt_font(fonts['font-name'])}\nfixed={qt_font(fonts['monospace-font-name'])}\n")


# A folha da Waybar que o pacote swaydesk-desktop instala, sem os @import (o embrulho abaixo os faz)
PACKAGE_WAYBAR_STYLE = "/usr/share/swaydesk-desktop/waybar/style.css"


def waybar_wrapper():
    """A folha da Waybar na sessão do pacote: as cores e as fontes geradas, depois a do pacote. Os caminhos
    das duas primeiras são relativos a este arquivo (~/.config/swaydesk-desktop/waybar/style.css).
    SWAYDESK_WAYBAR_STYLE troca a do pacote, para os testes."""
    base = os.environ.get("SWAYDESK_WAYBAR_STYLE") or PACKAGE_WAYBAR_STYLE
    base = base.replace("\\", "\\\\").replace('"', '\\"')
    return ("/* Gerado pelo sway-desktop-config: a Waybar da sessão do pacote (swaydesk-desktop) */\n"
            '@import url("../../waybar/accent.css");\n'
            '@import url("../../waybar/fonts.css");\n'
            f'@import url("{base}");\n')


ROFI_THEME = "sway-desktop/rofi/sway-desktop.rasi"  # no XDG_DATA_HOME; o sway-launcher o lê


def generated_files(colors, config=None, fonts=None):
    """O que depende do tema e não pode lê-lo: {caminho relativo à pasta de config: conteúdo}."""
    css = css_colors(colors)
    sway = (f"# Gerado pelo sway-desktop-config: mude a cor em ~/.config/sway-desktop/config.toml\n"
            f"client.focused {colors.accent} {colors.accent} {colors.on_accent} {colors.bright} {colors.accent}\n")
    config = config or Config()
    settings = gtk_settings(config)
    fonts = fonts or gtk_fonts()
    scheme = qt_color_scheme(config, colors)
    fonts_css = f"/* Gerado pelo sway-desktop-config: a fonte do sistema */\n{font_css(font_family(fonts['font-name']))}"
    return {
        "waybar/accent.css": css,
        "sway/config.d/00-accent.conf": sway,
        "sway/config.d/00-font.conf": sway_font(fonts),
        "sway/config.d/00-cursor.conf": sway_cursor(cursor_theme()),
        "waybar/fonts.css": fonts_css,
        "swaylock/config": render_template("swaylock.conf", colors),
        "gtk-3.0/settings.ini": settings,
        "gtk-4.0/settings.ini": settings,
        "sway-mimeapps.list": mimeapps(config),
        # Usados pelo Sway quando o Plasma não está instalado (sway-session): QT_QPA_PLATFORMTHEME=qt5ct
        "qt5ct/qt5ct.conf": qt_settings(5, config, fonts),
        "qt6ct/qt6ct.conf": qt_settings(6, config, fonts),
        "qt5ct/colors/sway-desktop.conf": scheme,
        "qt6ct/colors/sway-desktop.conf": scheme,
        # Sempre, mesmo no modo dotfiles: a sessão do pacote pode ser a próxima a abrir
        "swaydesk-desktop/waybar/style.css": waybar_wrapper(),
    }


GENERATED = "# Gerado pelo sway-desktop-config"
# Só estes são regravados se já existirem sem o cabeçalho: outro desktop pode cuidar deles. O KDE
# grava os settings.ini do GTK e, ao regravá-los, mantinha o tema do Sway no lugar do Breeze
SHARED_FILES = ("gtk-3.0/settings.ini", "gtk-4.0/settings.ini", "sway-mimeapps.list", "qt5ct/qt5ct.conf",
                "qt6ct/qt6ct.conf")
# Os settings.ini gerados antes do cabeçalho só tinham estas chaves
OLD_SETTINGS_KEYS = {"gtk-theme-name", "gtk-application-prefer-dark-theme", "gtk-icon-theme-name"}


def mimeapps(config):
    """Pastas abertas por outros apps no Sway (Firefox, xdg-open) vão para o gerenciador da config.
    Só vale com XDG_CURRENT_DESKTOP=sway: o GNOME e o KDE continuam com o deles. Sem isso, no Kubuntu
    as pastas abriam no Dolphin, que vence pela ordem."""
    desktop = get(config, "file_manager_desktop")
    return (f"{GENERATED}: mude o gerenciador em ~/.config/sway-desktop/config.toml (file_manager)\n"
            f"[Default Applications]\ninode/directory={desktop}.desktop\n")


def write_file(file, content):
    """Troca o caminho de uma vez, sem escrever através de um link para o repositório."""
    file.parent.mkdir(parents=True, exist_ok=True)
    tmp = file.with_name(f"{file.name}.tmp.{os.getpid()}")
    try:
        tmp.write_text(content, encoding="utf-8")
        os.replace(tmp, file)
    finally:
        tmp.unlink(missing_ok=True)


def generate(config):
    """Grava os arquivos. Devolve os compartilhados que ficaram como estavam, por serem de outro."""
    colors = palette(config)
    base = path().parents[1]
    kept = []
    for name, content in generated_files(colors, config).items():
        if name in SHARED_FILES and not ours(base / name):
            kept.append(name)
            continue
        write_file(base / name, content)
    # O tema do rofi do sway-launcher (Alt+Espaço)
    write_file(data_home() / ROFI_THEME, render_template("rofi.rasi", colors))
    return kept


def ours(file):
    """O arquivo não existe ou foi gerado por aqui, inclusive um settings.ini de antes do cabeçalho.
    Um link também: as instalações antigas ligavam os settings.ini ao repositório."""
    if Path(file).is_symlink():
        return True
    try:
        text = Path(file).read_text(encoding="utf-8")
    except FileNotFoundError:
        return True
    except (OSError, UnicodeDecodeError):
        return False
    if text.startswith(GENERATED):
        return True
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    return (Path(file).name == "settings.ini" and lines[:1] == ["[Settings]"]
            and {line.partition("=")[0] for line in lines[1:]} <= OLD_SETTINGS_KEYS)


def reload_sway():
    """Devolve se o Sway recarregou. Sem sessão do Sway, não há o que recarregar."""
    try:
        return subprocess.run(["swaymsg", "reload"], capture_output=True, timeout=5, check=False).returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False
