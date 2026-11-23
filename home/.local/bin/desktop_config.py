"""A config única do desktop (~/.config/swaydesk/config.toml), sem GTK.

Arquivo ausente ou inválido usa os padrões: nenhum widget pode deixar de abrir por causa dele.
"""
import os
import re
import shutil
import subprocess
import sys
import json
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
TEMPLATE_NAMES = ("swaylock.conf", "kitty-theme.conf", "rofi.rasi", "gtklock.css")  # os que o render_template usa
THEME_NAME = re.compile(r"[\w .+-]+")
# O tema de reserva, quando o pedido não está instalado: o adw-gtk3-dark (o pacote adw-gtk-theme), que segue a cor
# de destaque; sem ele, o Adwaita-dark do gnome-themes-extra
FALLBACK_THEME = "adw-gtk3-dark"
LAST_FALLBACK_THEME = "Adwaita-dark"
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
class Effects:
    """Os efeitos do SwayFX, do [effects] da config (o Effects do swaydesk_common::config). Cantos, sombras e o
    blur das janelas vêm ligados e discretos; o blur das camadas vem desligado (na VM, com virgl, ele deixou lixo
    visual na barra e nos popups) até um teste numa GPU real. Cada um liga ou desliga sozinho, e uma chave ausente
    fica com o padrão. O Sway puro os ignora."""
    corner_radius: int = 10  # os cantos das janelas e das camadas, em pixels; 0 desliga
    blur: bool = True  # o blur atrás das janelas transparentes
    shadows: bool = True  # a sombra das janelas
    layer_blur: bool = False  # o blur atrás da Waybar, da dock, da grade, dos popups, das notificações, do OSD e do
    # switcher; desligado por padrão
    dim_inactive: int = 0  # o escurecimento das janelas sem o foco, em centésimos (0.15 na config: 15); 0 desliga
    animation_duration_ms: int = 0  # as animações do SwayFX 0.6 (abrir e fechar janelas), em ms; 0 desliga


MAX_CORNER_RADIUS = 30  # mais que isso, os cantos comem o conteúdo das janelas
MAX_ANIMATION_DURATION_MS = 5000  # o limite do animation_duration_ms do SwayFX (5 segundos)


def parse_effects(data, warnings):
    """O [effects] da config; uma chave ruim vira aviso, e as outras valem."""
    effects = Effects()
    for key, value in data.items():
        if key not in {f.name for f in fields(Effects)}:
            warnings.append(f"config: unknown option: effects.{key}")
        elif key == "corner_radius":
            if type(value) is int and 0 <= value <= MAX_CORNER_RADIUS:
                effects = replace(effects, corner_radius=value)
            else:
                warnings.append(f"config: invalid effects.corner_radius (use 0 to {MAX_CORNER_RADIUS})")
        elif key == "dim_inactive":
            # 0 a 1, em float ou inteiro (0 e 1), guardado em centésimos, como no Rust
            if type(value) is float and 0.0 <= value <= 1.0:
                effects = replace(effects, dim_inactive=round(value * 100))
            elif type(value) is int and value in (0, 1):
                effects = replace(effects, dim_inactive=value * 100)
            else:
                warnings.append("config: invalid effects.dim_inactive (use 0.0 to 1.0)")
        elif key == "animation_duration_ms":
            if type(value) is int and 0 <= value <= MAX_ANIMATION_DURATION_MS:
                effects = replace(effects, animation_duration_ms=value)
            else:
                warnings.append(f"config: invalid effects.animation_duration_ms (use 0 to {MAX_ANIMATION_DURATION_MS})")
        elif type(value) is bool:
            effects = replace(effects, **{key: value})
        else:
            warnings.append(f"config: invalid effects.{key} (use true or false)")
    return effects


@dataclass(frozen=True)
class Config:
    locale: str = ""  # como "pt_BR"; vazio: o do ambiente
    weather_location: str = ""  # vazio: vem do IP
    holidays: str = ""  # o código do país, como "BR"; vazio: o do locale; "none": nenhum
    accent: str = DEFAULT_ACCENT  # como "#199ca8"
    # A cor tirada do papel de parede: o sway-theme accent-auto grava a cor em accent. Sem a chave (ou com um valor
    # inválido), ligado só se o accent é o DEFAULT_ACCENT: quem escolheu outra cor antes da chave fica com ela
    accent_auto: bool = True
    gtk_theme: str = "Mint-Y-Dark-Teal"
    icon_theme: str = "Papirus-Dark"
    folder_color: str = ""  # a cor das pastas do Papirus, como "teal"; vazio: a mais perto do destaque
    file_manager: str = ""  # comando, como "thunar"; vazio: o primeiro instalado de FILE_MANAGERS
    components: str = "auto"  # a versão dos componentes: auto (Rust onde houver), python ou rust
    shortcuts: str = "swaydesk"  # o perfil de atalhos, um dos SHORTCUT_PROFILES
    # Quando o popup Sistema mostra os perfis de energia, um dos POWER_IN_SYSTEM: "auto" (só sem bateria), "always"
    # ou "never"
    power_profiles_in_system: str = "auto"
    # Os componentes desligados no [enabled] (false), como "dock" ou "bar.weather". O que não está aqui está
    # ligado: a chave ausente liga, para um config antigo continuar como era (menos as do DEFAULT_OFF)
    enabled: frozenset = frozenset()
    # Os ligados com true no [enabled]: só contam para as chaves do DEFAULT_OFF. Não é uma chave do arquivo
    turned_on: frozenset = frozenset()
    effects: Effects = Effects()  # os efeitos do SwayFX ([effects])


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
    return config_dir() / "config.toml"


COMPONENTS = ("auto", "python", "rust")
# Os valores do power_profiles_in_system (o IN_SYSTEM do sway_power e do swaydesk_common::power)
POWER_IN_SYSTEM = ("auto", "always", "never")
# Os perfis de atalhos do shortcuts (o SHORTCUT_PROFILES do swaydesk_common::config): o nome no config e o
# mostrado, o da marca. Cada um é um sway/keys/<nome>.conf, incluído depois do keys/base.conf
SHORTCUT_PROFILES = {"swaydesk": "Swaydesk", "i3": "i3", "hyprland": "Hyprland", "niri": "niri", "dwm": "dwm"}
# As chaves do [enabled]: os componentes da sessão que podem ser desligados (o enabled() do Rust tem as mesmas)
ENABLED_KEYS = ("dock", "app_grid", "switcher", "notifications", "osd", "clipboard_history", "nightlight",
                "bing_wallpaper", "polkit_agent", "update_notifier", "displays", "autostart", "input_method")
# As que vêm desligadas: só um true (do usuário ou da camada do sistema) liga. O input_method (o fcitx5, para o ç
# no Chromium e no Electron) muda a digitação de todos os apps: num sistema que já existe, nada o liga sozinho; as
# ISOs ligam pelo SYSTEM_PATH. O DEFAULT_OFF do swaydesk_common::config
DEFAULT_OFF = ("input_method",)
# A camada do sistema: só o [enabled] dela é lido, por baixo do config do usuário (a chave do usuário vence).
# Nenhum pacote a traz; as ISOs a trazem. O SWAYDESK_SYSTEM_CONFIG troca o caminho nos testes
SYSTEM_PATH = "/etc/swaydesk/config.toml"
# As do [enabled.bar], com o módulo da Waybar de cada uma. Os workspaces, o sway/mode e a bandeja ficam sempre
BAR_MODULES = {
    "weather": "custom/weather", "clock": "custom/clock", "media": "custom/media", "recording": "custom/rec",
    "notifications": "custom/notification", "clipboard": "custom/clipboard", "language": "custom/language",
    "system": "custom/system", "brightness": "custom/brightness", "caffeine": "custom/caffeine",
    "bluetooth": "custom/bluetooth", "network": "custom/network", "volume": "custom/volume",
    "battery": "custom/battery", "power": "custom/power", "updates": "custom/updates",
}


def parse_enabled(table, warnings):
    """Os nomes desligados (false) e os ligados (true) do [enabled]. Um valor que não é true nem false, ou um
    nome desconhecido, avisa e fica com o padrão: um erro no arquivo nunca muda nada."""
    off, on = set(), set()
    if not isinstance(table, dict):
        warnings.append("config: invalid value in enabled")
        return frozenset(), frozenset()
    for key, value in table.items():
        if key == "bar" and isinstance(value, dict):
            for module, shown in value.items():
                if module not in BAR_MODULES:
                    warnings.append(f"config: unknown option: enabled.bar.{module}")
                elif not isinstance(shown, bool):
                    warnings.append(f"config: invalid value in enabled.bar.{module} (use true or false)")
                elif not shown:
                    off.add(f"bar.{module}")
        elif key not in ENABLED_KEYS:
            warnings.append(f"config: unknown option: enabled.{key}")
        elif not isinstance(value, bool):
            warnings.append(f"config: invalid value in enabled.{key} (use true or false)")
        else:
            (on if value else off).add(key)
    return frozenset(off), frozenset(on)


def known_component(name):
    """Se o nome é uma das chaves do [enabled] ("dock") ou do [enabled.bar] ("bar.weather")."""
    return name in ENABLED_KEYS or (name.startswith("bar.") and name[4:] in BAR_MODULES)


def enabled(config, name):
    """Se o componente está ligado. Um nome desconhecido conta como ligado."""
    if name in config.enabled:
        return False
    return name not in DEFAULT_OFF or name in config.turned_on


def with_system(config, system):
    """Põe por baixo o [enabled] da camada do sistema: cada chave que o usuário não deu fica com o valor dela."""
    given = config.enabled | config.turned_on
    return replace(config, enabled=config.enabled | (system.enabled - given),
                   turned_on=config.turned_on | (system.turned_on - given))


def parse_config(data):
    """Devolve a config e os avisos: um valor ruim não invalida os outros."""
    config, warnings = Config(), []
    auto_given = False
    known = {f.name for f in fields(Config)} - {"turned_on"}
    for key, value in data.items():
        if key not in known:
            warnings.append(f"config: unknown option: {key}")
        elif key == "enabled":
            off, on = parse_enabled(value, warnings)
            config = replace(config, enabled=off, turned_on=on)
        elif key == "effects" and isinstance(value, dict):
            config = replace(config, effects=parse_effects(value, warnings))
        elif key == "effects":
            warnings.append("config: invalid value in effects (use an [effects] table)")
        elif key == "accent_auto" and type(value) is bool:
            config = replace(config, accent_auto=value)
            auto_given = True
        elif key == "accent_auto":
            warnings.append("config: invalid value in accent_auto (use true or false)")
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
        elif key == "shortcuts" and value not in SHORTCUT_PROFILES:
            warnings.append(f"config: invalid shortcuts: {value} (use swaydesk, i3, hyprland, niri or dwm)")
        elif key == "power_profiles_in_system" and value not in POWER_IN_SYSTEM:
            warnings.append(f"config: invalid power_profiles_in_system: {value} (use auto, always or never)")
        elif key == "file_manager" and value and not COMMAND_NAME.fullmatch(value):
            warnings.append(f"config: invalid file manager: {value} (use the command name, like thunar)")
        else:
            config = replace(config, **{key: value})
    if not auto_given:
        config = replace(config, accent_auto=config.accent == DEFAULT_ACCENT)
    return config, warnings


def load_config(file):
    try:
        with open(file, "rb") as f:
            return parse_config(tomllib.load(f))
    except FileNotFoundError:
        return Config(), []
    except (OSError, tomllib.TOMLDecodeError, UnicodeDecodeError) as err:
        return Config(), [f"config: could not read {file}: {err}"]


def system_path():
    """O caminho da camada do sistema (o SYSTEM_PATH, ou o SWAYDESK_SYSTEM_CONFIG dos testes)."""
    return Path(os.environ.get("SWAYDESK_SYSTEM_CONFIG") or SYSTEM_PATH)


def load_system(file):
    """A camada do sistema: só o [enabled]; outra chave avisa e é ignorada. Sem o arquivo, vazia."""
    try:
        with open(file, "rb") as f:
            data = tomllib.load(f)
    except FileNotFoundError:
        return Config(), []
    except (OSError, tomllib.TOMLDecodeError, UnicodeDecodeError) as err:
        return Config(), [f"config: could not read {file}: {err}"]
    warnings = [f"config: {file}: only [enabled] is read, not {key}" for key in data if key != "enabled"]
    if "enabled" not in data:
        return Config(), warnings
    off, on = parse_enabled(data["enabled"], warnings)
    return Config(enabled=off, turned_on=on), warnings


def load():
    """A config do usuário, com a camada do sistema por baixo do [enabled]; os avisos vão para o stderr."""
    config, warnings = load_config(path())
    system, system_warnings = load_system(system_path())
    for warning in warnings + system_warnings:
        print(warning, file=sys.stderr)
    return with_system(config, system)


def effective_locale(config, env=os.environ):
    """O locale da config ou, sem ele, o de LC_ALL, LC_TIME ou LANG, nessa ordem. C e POSIX: vazio."""
    if config.locale:
        return config.locale
    for name in ("LC_ALL", "LC_TIME", "LANG"):
        if env.get(name):
            m = LOCALE.match(env[name])
            return m.group() if m else ""
    return ""


def time_locales(config, env=os.environ, interface=""):
    """Os locales da data, na ordem de tentar: o efetivo (effective_locale) e, se ele não estiver gerado, o da
    língua dos textos (interface, como "es_ES"). Só nomes xx_YY, sem repetir. O C escolhido (LC_TIME=C.UTF-8
    para a data ISO, LC_ALL=POSIX) fica no C: vazio; sem nada definido, a língua dos textos."""
    out = []
    first = effective_locale(config, env)
    if first:
        out.append(first)
    elif any(env.get(name) for name in ("LC_ALL", "LC_TIME", "LANG")):
        return out
    m = LOCALE.match(interface or "")
    if m and m.group() not in out:
        out.append(m.group())
    return out


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
    if key in ("weather_location", "holidays", "accent", "gtk_theme", "icon_theme", "folder_color", "components",
               "shortcuts"):
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


def state_home():
    return _xdg("XDG_STATE_HOME", ".local/state")


def _xdg(name, fallback):
    """Um XDG_* absoluto, ou o padrão: um relativo é ignorado, como pede a especificação (os caminhos e os links
    da migração dependeriam do cwd)."""
    value = os.environ.get(name) or ""
    return Path(value) if os.path.isabs(value) else Path.home() / fallback


# As pastas com o nome novo (swaydesk) e a migração das velhas (sway-desktop): o mesmo que o
# swaydesk_common::dirs do Rust, com os mesmos casos (a explicação completa está lá).
# Leitura e escrita: vale a velha enquanto ela existe e não é o nosso link (uma pasta de verdade ou um link do
# usuário); senão, a nova. A migração só roda no login (setup, sway-session) e no install.sh, nunca no meio da
# sessão.
NEW_DIR = "swaydesk"
OLD_DIR = "sway-desktop"


def dir_pairs():
    """Os pares (velho, novo) na ordem da migração: as pastas-mãe antes da aninhada da Waybar."""
    config, data, state = _xdg("XDG_CONFIG_HOME", ".config"), _xdg("XDG_DATA_HOME", ".local/share"), state_home()
    return [
        (config / OLD_DIR, config / NEW_DIR),
        (data / OLD_DIR, data / NEW_DIR),
        (state / OLD_DIR, state / NEW_DIR),
        (config / "swaydesk-desktop/waybar", config / NEW_DIR / "waybar"),
    ]


def link_target(old, new):
    """O caminho de new relativo à pasta de old: o alvo do link que fica no lugar do velho."""
    return Path(os.path.relpath(new, old.parent))


def _is_our_link(old, new):
    try:
        target = Path(os.readlink(old))
    except OSError:
        return False
    return target in (link_target(old, new), new)


def _is_real_dir(path):
    return path.is_dir() and not path.is_symlink()


def pick_dir(old, new):
    """A pasta que vale para o par: a velha enquanto existe e não é o nosso link; senão, a nova."""
    return old if old.is_dir() and not _is_our_link(old, new) else new


def config_dir():
    config = _xdg("XDG_CONFIG_HOME", ".config")
    return pick_dir(config / OLD_DIR, config / NEW_DIR)


def data_dir():
    data = _xdg("XDG_DATA_HOME", ".local/share")
    return pick_dir(data / OLD_DIR, data / NEW_DIR)


def state_dir():
    return pick_dir(state_home() / OLD_DIR, state_home() / NEW_DIR)


def waybar_dir():
    """Para onde o nosso link aponta (mesmo que o par da config tenha ficado no velho); senão, a velha de verdade;
    senão, a de dentro da config em uso."""
    config = _xdg("XDG_CONFIG_HOME", ".config")
    old, new = config / "swaydesk-desktop/waybar", config / NEW_DIR / "waybar"
    if _is_our_link(old, new):
        return new
    if old.is_dir():
        return old
    return config_dir() / "waybar"


def cache_dir():
    """O cache não migra: recomeça vazio no novo."""
    return _xdg("XDG_CACHE_HOME", ".cache") / NEW_DIR


MAX_ROUNDS = 2  # quantas vezes o velho pode ser recriado por fora entre o esvaziar e o link
CONFIG_PAIR, WAYBAR_PAIR = 0, 3  # os índices em dir_pairs()


def _symlink_or_merge(old, new, round_=0):
    try:
        os.symlink(link_target(old, new), old)
        return "moved"
    except FileExistsError as err:
        # Alguém de fora da trava recriou o velho entre o rename e o link: os itens dele vão para o novo
        if round_ < MAX_ROUNDS:
            return _merge(old, new, round_ + 1)
        return f"failed: {old}: {err}"
    except OSError as err:
        return f"failed: {old}: {err}"


def _merge(old, new, round_=0):
    try:
        names = sorted(p.name for p in old.iterdir())
    except OSError as err:
        return f"failed: {old}: {err}"
    both = [n for n in names if os.path.lexists(new / n)]
    if both:
        return both
    for moved, name in enumerate(names):
        try:
            os.rename(old / name, new / name)
        except OSError as err:
            # Desfaz o que já foi: com o velho ainda pasta, o helper continua nele, e os itens movidos sumiriam
            message = f"failed: {old / name}: {err}"
            for back in reversed(names[:moved]):
                try:
                    os.rename(new / back, old / back)
                except OSError as err2:
                    message += f"; and {new / back} could not go back: {err2}"
            return message
    try:
        old.rmdir()
    except OSError as err:
        return f"failed: {old}: {err}"
    return _symlink_or_merge(old, new, round_)


def migrate_pair(old, new):
    """Migra um par: "nothing", "moved", a lista dos itens que existem nos dois, ou "failed: …"."""
    if not _is_real_dir(old):
        return "nothing"
    if not os.path.lexists(new):
        try:
            new.parent.mkdir(parents=True, exist_ok=True)
            os.rename(old, new)
        except OSError as err:
            return f"failed: {old} → {new}: {err}"
        return _symlink_or_merge(old, new)
    if not _is_real_dir(new):
        return f"failed: {new}: not a folder"
    return _merge(old, new)


def migrate_dirs():
    """Migra todos os pares sob a trava (a mesma do Rust) e devolve uma linha por par que não migrou de vez."""
    import fcntl
    lock = _xdg("XDG_CACHE_HOME", ".cache") / "swaydesk-migrate.lock"
    try:
        lock.parent.mkdir(parents=True, exist_ok=True)
        handle = open(lock, "a")
        fcntl.flock(handle, fcntl.LOCK_EX)
    except OSError as err:
        return [f"folders not migrated: lock {lock}: {err}"]
    messages = []
    config_in_use = True
    with handle:
        for index, (old, new) in enumerate(dir_pairs()):
            # A pasta da Waybar só vai para dentro da config nova se a config nova é a que vale
            if index == WAYBAR_PAIR and not config_in_use:
                if _is_real_dir(old):
                    messages.append(f"{old} kept: {dir_pairs()[CONFIG_PAIR][0]} stays in use")
                continue
            result = migrate_pair(old, new)
            if index == CONFIG_PAIR:
                config_in_use = result in ("nothing", "moved") and not _is_real_dir(old)
            if isinstance(result, list):
                messages.append(f"{old} kept: {', '.join(result)} also exists in {new}; the old folder stays in use"
                                f" until you merge them by hand")
            elif result.startswith("failed: "):
                messages.append(f"{old} not migrated: {result.removeprefix('failed: ')}")
    return messages


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


def snap_roots():
    """Onde os snaps ficam montados: /snap (Ubuntu, Debian) ou /var/lib/snapd/snap (Arch, Fedora).
    SWAY_SNAP_ROOTS (separados por ':') troca, nos testes."""
    roots = os.environ.get("SWAY_SNAP_ROOTS")
    if roots is None:
        return [Path("/snap"), Path("/var/lib/snapd/snap")]
    return [Path(root) for root in roots.split(":") if root]


def snap_theme_prompt():
    """Se o snapd-desktop-integration está instalado (o Ubuntu Desktop). Ele lê o gtk-theme do dconf e, com um tema
    que os snaps não têm (o adw-gtk3-dark), avisa "Some required themes are missing" a cada login e abre o polkit ao
    abrir um snap, para instalar o snap de terceiros do tema. O Adwaita-dark vem com o gtk-common-themes. Só o snapd
    não basta: sem o snapd-desktop-integration, ninguém pede o tema. Uma conta que já tem o adw-gtk3-dark gravado fica
    com ele se o snap vier depois, de propósito (não dá para distinguir de uma escolha no app Tema)."""
    return any((root / "snapd-desktop-integration").is_dir() for root in snap_roots())


def fallback_theme():
    """O tema de reserva que existe: o adw-gtk3-dark; o Adwaita-dark sem ele (os dotfiles sem o pacote) ou com o
    snapd-desktop-integration (como na 0.1.3.1)."""
    if not snap_theme_prompt() and theme_installed(FALLBACK_THEME):
        return FALLBACK_THEME
    return LAST_FALLBACK_THEME


def effective_gtk_theme(config):
    return config.gtk_theme if theme_installed(config.gtk_theme) else fallback_theme()


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


def gtk_settings(config, prefer_dark=False):
    # O do GTK 3: prefer-dark sempre 0. Os temas escuros já são escuros, e o GTK só lê esse valor ao iniciar:
    # com 1, uma janela aberta seguiria na variante escura depois da troca para um tema claro
    return (f"{GENERATED}: mude o tema com sway-theme\n"
            "[Settings]\n"
            f"gtk-theme-name={effective_gtk_theme(config)}\n"
            f"gtk-application-prefer-dark-theme={int(prefer_dark)}\n"
            f"gtk-icon-theme-name={effective_icon_theme(config)}\n")


def gtk4_settings(config):
    """O do GTK 4: o mesmo, menos com o adw-gtk3 escuro. O CSS de GTK 4 dele só escurece dentro de
    @media (prefers-color-scheme: dark), que o GTK 4.20 e o 4.22 (Ubuntu 26.04) seguem pelo prefer-dark. O mesmo
    do gtk4_settings do rust/theme/src/desktop.rs."""
    theme = effective_gtk_theme(config)
    return gtk_settings(config, theme.startswith("adw-gtk3") and is_dark(theme))


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


# As camadas do layer-shell com o blur do SwayFX (layer_effects), pelo namespace: a Waybar, a dock, os popups da
# Waybar, as notificações, o OSD, o switcher e os painéis de tela inteira (a grade e o menu de sessão). Ficam de
# fora o popup-click-catcher (de tela inteira e quase transparente: borraria a tela toda), o hotspot da dock e a
# captura (quem escolhe a região ou a janela precisa vê-la nítida). O BLUR_LAYERS do swaydesk-theme
BLUR_LAYERS = ("waybar", "sway-dock", "waybar-audio", "waybar-battery-popup", "waybar-bluetooth", "waybar-brightness",
               "waybar-caffeine", "waybar-calendar", "waybar-clipboard-popup", "waybar-media", "waybar-network",
               "waybar-notifications-popup", "waybar-system", "waybar-weather", "sway-notifd", "sway-osd-bar",
               "sway-switcher", "sway-drawer", "session-menu")


def is_swayfx():
    """Se o sway instalado é o SwayFX ("swayfx version ..." no sway --version). SWAYDESK_SWAYFX=1 ou 0 decide
    no lugar dele, para os testes."""
    if "SWAYDESK_SWAYFX" in os.environ:
        return os.environ["SWAYDESK_SWAYFX"] == "1"
    try:
        out = subprocess.run(["sway", "--version"], capture_output=True, text=True, timeout=5).stdout
    except (OSError, subprocess.SubprocessError):
        return False
    return out.startswith("swayfx")


PACKAGE_KEYS_DIR = Path("/usr/share/swaydesk-desktop/sway/keys")


def keys_dir(env=os.environ):
    """A pasta dos perfis de atalhos desta sessão (o keys_dir do swaydesk_common::config). Na sessão do pacote, a
    do pacote. Nos dotfiles, a pasta keys ao lado do config do Sway de verdade (o link ~/.config/sway/config
    resolvido, no repositório): vale mesmo sem os links do install.sh, depois de um git pull. Sem ela, a de
    ~/.config/sway/keys ou a do pacote, a primeira que tiver o base.conf."""
    if env.get("SWAYDESK_SESSION") == "package":
        return PACKAGE_KEYS_DIR
    sway = Path(env.get("XDG_CONFIG_HOME") or Path(env.get("HOME", "/")) / ".config") / "sway"
    candidates = []
    try:
        candidates.append((sway / "config").resolve().parent / "keys")
    except (OSError, RuntimeError):
        pass
    candidates += [sway / "keys", PACKAGE_KEYS_DIR]
    return next((d for d in candidates if (d / "base.conf").is_file()), candidates[0])


def sway_keys(config, keys=None):
    """O config.d/00-keys.conf: os atalhos de todos os perfis (base.conf) e os do perfil do shortcuts, com o caminho
    inteiro da pasta desta sessão (keys_dir). O sway-keys o regrava a cada início e reload da sessão."""
    keys = keys or keys_dir()
    return f'{KEYS_HEAD}\ninclude "{keys}/base.conf"\ninclude "{keys}/{config.shortcuts}.conf"\n'


KEYS_HEAD = "# Gerado pelo sway-desktop-config: os atalhos do perfil; mude o shortcuts em ~/.config/swaydesk/config.toml"


def sway_effects(effects, swayfx):
    """O config.d/10-effects.conf: no SwayFX, os efeitos do [effects]; no Sway puro, só um comentário (as
    diretivas do SwayFX dariam erro de config). O animation_duration_ms só entra quando não é 0: o SwayFX 0.5
    não conhece a diretiva, e 0 já é o padrão do 0.6."""
    if not swayfx:
        return (f"{GENERATED}: o Sway sem os efeitos; com o SwayFX (no Arch, pelo AUR), o [effects] de\n"
                "# ~/.config/swaydesk/config.toml vale aqui\n")
    on = lambda flag: "enable" if flag else "disable"  # noqa: E731
    text = (f"{GENERATED}: os efeitos do SwayFX; mude em ~/.config/swaydesk/config.toml ([effects])\n"
            f"corner_radius {effects.corner_radius}\nblur {on(effects.blur)}\n")
    if effects.blur:
        text += "blur_passes 2\nblur_radius 5\n"
    text += f"shadows {on(effects.shadows)}\n"
    if effects.shadows:
        text += "shadow_blur_radius 20\nshadow_color #0000004D\n"  # uma sombra leve: preta a 30%
    text += f"default_dim_inactive {effects.dim_inactive // 100}.{effects.dim_inactive % 100:02d}\n"
    if effects.animation_duration_ms:
        text += f"animation_duration_ms {effects.animation_duration_ms}\n"
    if effects.layer_blur:
        # O blur só onde a camada desenha: as margens transparentes dos popups ficam limpas
        text += "".join(f'layer_effects "{layer}" blur enable; blur_ignore_transparent enable; '
                        f"corner_radius {effects.corner_radius}\n" for layer in BLUR_LAYERS)
    return text


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
    das duas primeiras são relativos a este arquivo (~/.config/swaydesk/waybar/style.css, ou a pasta velha: a
    profundidade é a mesma). SWAYDESK_WAYBAR_STYLE troca a do pacote, para os testes."""
    base = os.environ.get("SWAYDESK_WAYBAR_STYLE") or PACKAGE_WAYBAR_STYLE
    base = base.replace("\\", "\\\\").replace('"', '\\"')
    return ("/* Gerado pelo sway-desktop-config: a Waybar da sessão do pacote (swaydesk-desktop) */\n"
            '@import url("../../waybar/accent.css");\n'
            '@import url("../../waybar/fonts.css");\n'
            f'@import url("{base}");\n')


ROFI_THEME = "rofi/sway-desktop.rasi"  # na data_dir(); o sway-launcher o lê
GTKLOCK_STYLE = "gtklock/style.css"  # na data_dir(); o sway-gtklock o passa com -s


def generated_files(colors, config=None, fonts=None):
    """O que depende do tema e não pode lê-lo: {caminho relativo à pasta de config: conteúdo}."""
    css = css_colors(colors)
    sway = (f"# Gerado pelo sway-desktop-config: mude a cor em ~/.config/swaydesk/config.toml\n"
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
        "sway/config.d/00-keys.conf": sway_keys(config),
        "sway/config.d/10-effects.conf": sway_effects(config.effects, is_swayfx()),
        "waybar/fonts.css": fonts_css,
        "swaylock/config": render_template("swaylock.conf", colors),
        "gtk-3.0/settings.ini": settings,
        "gtk-4.0/settings.ini": gtk4_settings(config),
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
GENERATED_CSS = "/* Gerado pelo sway-desktop-config"  # o mesmo cabeçalho num CSS (os gtk.css da cor de destaque)
# Os gtk.css da cor de destaque: o GTK os lê em toda sessão, não só no Sway. Só são gravados sem outro desktop
# instalado (other_desktops) e só se não existem ou são daqui (um link é do usuário)
GTK_CSS_FILES = ("gtk-3.0/gtk.css", "gtk-4.0/gtk.css")
# Os desktops que têm a cor de destaque deles: o começo do nome do .desktop da sessão, ou um dos DesktopNames
OTHER_DESKTOPS = ("gnome", "ubuntu", "plasma", "kde", "cinnamon", "xfce", "mate", "budgie", "lxqt", "lxde", "cosmic",
                  "pantheon", "deepin", "unity", "enlightenment")
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
    return (f"{GENERATED}: mude o gerenciador em ~/.config/swaydesk/config.toml (file_manager)\n"
            f"[Default Applications]\ninode/directory={desktop}.desktop\n")


WAYBAR_GENERATED = "config.jsonc"  # na waybar_dir(), como o style.css do pacote
WAYBAR_PREFIX = "swaydesk-desktop/waybar/"  # os generated_files que vão para a waybar_dir()


def generated_path(base, name):
    """Onde vai um dos generated_files: na pasta de config, menos os da Waybar do pacote (a waybar_dir())."""
    return waybar_dir() / name.removeprefix(WAYBAR_PREFIX) if name.startswith(WAYBAR_PREFIX) else base / name


def strip_jsonc(text):
    """O JSONC da Waybar sem os comentários (// e /* */ fora das strings), para o json.loads."""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append(text[i:j + 1])
            i = j + 1
        elif text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


# O módulo das atualizações do sistema (o sway-updates), só onde há o checkupdates do pacman-contrib: o Arch
UPDATES_MODULE = "custom/updates"


def waybar_config(template_text, config, checkupdates=None):
    """O config da Waybar sem os módulos desligados no [enabled.bar], em JSON. ValueError se o modelo não
    for um JSONC válido: quem chama sobe a Waybar com o modelo. Sem o checkupdates (None: procura no PATH),
    sai também o custom/updates: no Debian e no Ubuntu a loja avisa do apt."""
    data = json.loads(strip_jsonc(template_text))
    off = {BAR_MODULES[name[4:]] for name in config.enabled if name.startswith("bar.")}
    if checkupdates is None:
        checkupdates = shutil.which("checkupdates") is not None
    # Com o update_notifier = false, o ícone some junto com a verificação: sem ela, o cache ficaria velho
    if not checkupdates or not enabled(config, "update_notifier"):
        off.add(UPDATES_MODULE)
    bars = data if isinstance(data, list) else [data]
    for bar in bars:
        if not isinstance(bar, dict):
            raise ValueError("a bar is not an object")
        for side in ("modules-left", "modules-center", "modules-right"):
            if isinstance(bar.get(side), list):
                bar[side] = [m for m in bar[side] if m not in off]
    return ("// Gerado pelo sway-desktop-config a partir do modelo, sem os módulos desligados no [enabled.bar]\n"
            + json.dumps(data, indent=2, ensure_ascii=False) + "\n")


def waybar_template():
    """O modelo da Waybar dos dotfiles: o ~/.config/waybar/config.jsonc."""
    return config_home() / "waybar" / "config.jsonc"


def write_waybar(config, template=None):
    """Grava o config da Waybar filtrado e devolve o caminho. OSError ou ValueError se não deu."""
    template = Path(template) if template else waybar_template()
    out = waybar_dir() / WAYBAR_GENERATED
    write_file(out, waybar_config(template.read_text(encoding="utf-8"), config))
    return out


def write_file(file, content):
    """Troca o caminho de uma vez, sem escrever através de um link para o repositório."""
    file.parent.mkdir(parents=True, exist_ok=True)
    tmp = file.with_name(f"{file.name}.tmp.{os.getpid()}")
    try:
        tmp.write_text(content, encoding="utf-8")
        os.replace(tmp, file)
    finally:
        tmp.unlink(missing_ok=True)


def gtk_css_files(colors):
    """Os gtk.css da cor de destaque. O GTK 3 lê as cores com nome (o adw-gtk3 tira o destaque do accent_bg_color; o
    Adwaita e o Mint-Y não o usam). O GTK 4 lê as variáveis do libadwaita 1.6 em diante, e o adw-gtk3 do GTK 4
    também. O GTK 3 não entende o :root: os dois são diferentes. O gtk_css_files do desktop.rs.
    Mudar o formato? O gtk_css_ours só reconhece o atual: aceite lá também o antigo, senão os gerados pela versão
    anterior viram do usuário para sempre."""
    head = f"{GENERATED_CSS}: a cor de destaque do app Tema. Mudar este arquivo desliga a geração */\n"
    named = f"@define-color accent_bg_color {colors.accent};\n@define-color accent_fg_color {colors.on_accent};\n"
    return {
        "gtk-3.0/gtk.css": head + named,
        "gtk-4.0/gtk.css": (f"{head}:root {{\n  --accent-bg-color: {colors.accent};\n"
                            f"  --accent-fg-color: {colors.on_accent};\n}}\n{named}"),
    }


def session_dirs():
    """As pastas das sessões instaladas: wayland-sessions e xsessions de cada XDG_DATA_DIRS."""
    dirs = os.environ.get("XDG_DATA_DIRS") or "/usr/local/share:/usr/share"
    return [Path(d) / kind for d in dirs.split(":") if d for kind in ("wayland-sessions", "xsessions")]


def is_other_desktop(stem, desktop_names):
    """A sessão é de um desktop com a própria cor de destaque: pelo nome do arquivo ou pelos DesktopNames."""
    stem = stem.lower()
    tokens = {token.strip().lower() for token in re.split(r"[;:]", desktop_names)}
    return any(stem.startswith(name) for name in OTHER_DESKTOPS) or bool(tokens & set(OTHER_DESKTOPS))


def other_desktops_in(dirs):
    """Os outros desktops instalados (o nome do .desktop de cada sessão), em ordem e sem repetir."""
    found = set()
    for folder in dirs:
        try:
            entries = list(Path(folder).iterdir())
        except OSError:
            continue
        for entry in entries:
            if entry.suffix != ".desktop":
                continue
            try:
                text = entry.read_bytes().decode("utf-8", errors="replace")
            except OSError:
                text = ""
            match = re.search(r"(?m)^DesktopNames=(.*)$", text)
            if is_other_desktop(entry.stem, match.group(1).strip() if match else ""):
                found.add(entry.stem)
    return sorted(found)


def other_desktops():
    return other_desktops_in(session_dirs())


def gtk_css_ours(file, name):
    """O gtk.css não existe ou é, byte a byte, o que geramos com alguma cor: qualquer mudança (uma regra a mais
    embaixo do cabeçalho) o faz ser do usuário. Um link também é dele: o projeto nunca ligou estes arquivos."""
    file = Path(file)
    if file.is_symlink():
        return False
    try:
        text = file.read_bytes().decode("utf-8")
    except FileNotFoundError:
        return True
    except (OSError, UnicodeDecodeError):
        return False
    colors = {}
    for line in text.splitlines():
        for key in ("accent_bg_color", "accent_fg_color"):
            prefix = f"@define-color {key} "
            if key not in colors and line.startswith(prefix) and line.endswith(";"):
                colors[key] = line[len(prefix):-1]
    if len(colors) != 2:
        return False
    generated = gtk_css_files(Palette(accent=colors["accent_bg_color"], hover="", bright="", soft="",
                                      on_accent=colors["accent_fg_color"]))
    return generated.get(name) == text


def sync_gtk_css(base, colors, others):
    """Grava os gtk.css da cor de destaque, ou tira os daqui com outro desktop instalado: um gravado antes de o GNOME
    chegar não pode ficar mandando a cor nele. Devolve os que são de outro e ficaram como estavam."""
    kept = []
    for name, content in gtk_css_files(colors).items():
        file = Path(base) / name
        if not gtk_css_ours(file, name):
            kept.append(name)
        elif not others:
            write_file(file, content)
        elif file.is_file():
            file.unlink()
    return kept


def generate(config):
    """Grava os arquivos. Devolve os compartilhados que ficaram como estavam, por serem de outro."""
    colors = palette(config)
    base = path().parents[1]
    kept = []
    for name, content in generated_files(colors, config).items():
        if name in SHARED_FILES and not ours(base / name):
            kept.append(name)
            continue
        write_file(generated_path(base, name), content)
    kept += sync_gtk_css(base, colors, other_desktops())
    # O tema do rofi do sway-launcher (Alt+Espaço)
    write_file(data_dir() / ROFI_THEME, render_template("rofi.rasi", colors))
    write_file(data_dir() / GTKLOCK_STYLE, render_template("gtklock.css", colors))
    # A Waybar sem os módulos desligados. Sem o modelo, ou com ele quebrado, o sway-waybar usa o modelo
    try:
        write_waybar(config)
    except (OSError, ValueError, UnicodeDecodeError) as err:
        print(f"sway-desktop-config: waybar config not generated: {err}", file=sys.stderr)
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


# O papel de parede que o config do Sway lê (output * bg). Sem ele, o reload abre o swaynag: "unable to access
# background file". O install.sh copia a imagem lisa do home-copy; o bing-wallpaper troca o link pela do dia
WALLPAPER = Path("~/.local/share/wallpaper/current.jpg")
# A imagem lisa: a do home-copy, no repositório (os dotfiles); ou a do skel do pacote, que o install-tree.sh
# instala a partir do mesmo home-copy (este módulo também vai no pacote, em /usr/libexec/swaydesk)
WALLPAPER_SEEDS = (Path(__file__).resolve().parents[3] / "home-copy/.local/share/wallpaper/current.jpg",
                   Path("/usr/share/swaydesk-desktop/skel/.local/share/wallpaper/current.jpg"))


def ensure_wallpaper(seeds=WALLPAPER_SEEDS):
    """Põe a imagem lisa no lugar de um papel de parede que sumiu (ou de um link quebrado), como o install.sh no
    primeiro uso. Devolve se repôs."""
    target = WALLPAPER.expanduser()
    seed = next((path for path in seeds if path.is_file()), None)
    if target.exists() or seed is None:
        return False
    try:
        if target.is_symlink():
            target.unlink()
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(seed, target)
    except OSError:
        return False
    return True


def reload_sway():
    """Devolve se o Sway recarregou. Sem sessão do Sway, não há o que recarregar. Antes, o papel de parede que o
    config lê: sem ele, o Sway abriria o swaynag."""
    ensure_wallpaper()
    try:
        return subprocess.run(["swaymsg", "reload"], capture_output=True, timeout=5, check=False).returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False
