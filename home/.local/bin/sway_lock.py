"""A lógica do bloqueio de tela, sem GTK: hora e data, foto e nome do usuário, e o fundo desfocado.

O sway-lock-gtk (Python) usa este módulo, e o swaydesk-lock (Rust) porta os mesmos testes. O fundo
desfocado nunca é gerado no caminho do bloqueio: o locker só lê o cache, e quem gera é o sway-theme e a
troca de papel de parede (o prazo do sway-lock fica bem abaixo dos 5 s do logind).
"""
import hashlib
import os
import pwd
import re
from pathlib import Path

WALLPAPER = Path("~/.local/share/wallpaper/current.jpg")
ACCOUNTS = Path("/var/lib/AccountsService")
CACHE_NAME = "swaydesk"  # o cache não migra da sway-desktop: recomeça vazio (o mesmo do Rust)
# O desfoque: reduz a 1/BLUR_FACTOR, amplia de novo, e escurece com preto a DARKEN (0 a 255)
BLUR_FACTOR = 24
DARKEN = 110


def time_text(now):
    return now.strftime("%H:%M")


def date_format(time_locale):
    """O formato da data por extenso segue a língua em que o strftime escreve: o "de" só existe em
    português e em espanhol ("jueves, 20 de noviembre"). time_locale: o locale efetivo
    (desktop_config.effective_locale), como "pt_BR"."""
    if time_locale.startswith(("pt", "es")):
        return "%A, %-d de %B"
    return "%A, %B %-d"


def set_time_locale(candidates):
    """Põe no LC_TIME o primeiro destes locales (como "pt_BR") que estiver gerado, com o .UTF-8. Devolve o que
    ficou ("" se nenhum: o C, a data em inglês) e os que faltaram, para quem chama avisar."""
    import locale
    missing = []
    for candidate in candidates:
        try:
            locale.setlocale(locale.LC_TIME, f"{candidate}.UTF-8")
        except locale.Error:
            missing.append(candidate)
            continue
        return candidate, missing
    return "", missing


def missing_time_locale_line(missing, used):
    """A linha do log quando o locale da data não está gerado; None sem nenhum faltando."""
    if not missing:
        return None
    names = ", ".join(f"{m}.UTF-8" for m in missing)
    verb = "is" if len(missing) == 1 else "are"
    return f"{names} {verb} not generated: the date uses {f'{used}.UTF-8' if used else 'C'}"


def date_text(now, time_locale):
    """A data por extenso. O LC_TIME do processo já está no locale (quem chama faz o setlocale)."""
    text = now.strftime(date_format(time_locale))
    return text[:1].upper() + text[1:]


def real_name(user, accounts=ACCOUNTS):
    """O RealName do AccountsService, o primeiro campo do GECOS ou o login, nessa ordem.
    user: o pwd.struct_passwd do getpwuid."""
    try:
        text = (accounts / "users" / user.pw_name).read_text(encoding="utf-8", errors="replace")
    except OSError:
        text = ""
    for line in text.splitlines():
        key, _, value = line.partition("=")
        if key.strip() == "RealName" and value.strip():
            return value.strip()
    return (user.pw_gecos.split(",")[0] or user.pw_name).strip() or user.pw_name


def photo(user, accounts=ACCOUNTS):
    """O ~/.face ou o ícone do AccountsService, se for um arquivo legível; senão None (as iniciais)."""
    for path in (Path(user.pw_dir) / ".face", accounts / "icons" / user.pw_name):
        try:
            if path.is_file() and os.access(path, os.R_OK) and path.stat().st_size > 0:
                return path
        except OSError:
            continue
    return None


def initials(name):
    """Até duas letras: a primeira do primeiro nome e a do último. "" se não houver letras."""
    words = [w for w in re.split(r"\s+", name.strip()) if w and w[0].isalnum()]
    if not words:
        return ""
    letters = words[0][0] + (words[-1][0] if len(words) > 1 else "")
    return letters.upper()


def current_user():
    """Do getpwuid, nunca do $USER."""
    return pwd.getpwuid(os.getuid())


def cache_dir(env=os.environ):
    base = env.get("XDG_CACHE_HOME") or str(Path(env.get("HOME") or Path.home()) / ".cache")
    return Path(base) / CACHE_NAME


def blur_key(wallpaper, width, height):
    """A chave do fundo desfocado: o arquivo de verdade (o current.jpg é um link para a imagem do dia),
    o tamanho, a data de modificação e o tamanho do monitor. None se o papel de parede não existir.
    Sem ler o arquivo inteiro: o locker calcula a chave no caminho do bloqueio."""
    try:
        real = Path(wallpaper).expanduser().resolve(strict=True)
        st = real.stat()
    except OSError:
        return None
    raw = f"{real}\0{st.st_size}\0{st.st_mtime_ns}\0{int(width)}x{int(height)}"
    return hashlib.sha256(raw.encode("utf-8", "surrogateescape")).hexdigest()[:32]


def blur_path(wallpaper, width, height, env=os.environ):
    key = blur_key(wallpaper, width, height)
    return None if key is None else cache_dir(env) / f"lock-{key}.png"


def cached_blur(wallpaper, width, height, env=os.environ):
    """O fundo desfocado já pronto, ou None. Nunca gera: é o que o locker chama."""
    path = blur_path(wallpaper, width, height, env)
    return path if path is not None and path.is_file() else None


def cover_size(src_w, src_h, width, height):
    """O tamanho para a imagem cobrir o monitor sem faixas, mantendo a proporção (como o fill)."""
    scale = max(width / src_w, height / src_h)
    return max(width, round(src_w * scale)), max(height, round(src_h * scale))


def make_blur(wallpaper, width, height, env=os.environ, keep=4):
    """Gera o fundo desfocado do monitor, se faltar, e apaga os mais velhos (fica com os `keep` mais
    novos, que cobrem alguns monitores e o papel de parede anterior). Devolve o caminho, ou None sem papel de parede. Usa o GdkPixbuf,
    sem GTK: reduzir e ampliar duas vezes é barato e basta para o fundo."""
    path = blur_path(wallpaper, width, height, env)
    if path is None:
        return None
    if path.is_file():
        return path
    import gi

    gi.require_version("GdkPixbuf", "2.0")
    from gi.repository import GdkPixbuf

    source = GdkPixbuf.Pixbuf.new_from_file(str(Path(wallpaper).expanduser()))
    cw, ch = cover_size(source.get_width(), source.get_height(), width, height)
    image = source.scale_simple(cw, ch, GdkPixbuf.InterpType.BILINEAR)
    image = image.new_subpixbuf((cw - width) // 2, (ch - height) // 2, width, height).copy()
    small_w, small_h = max(1, width // BLUR_FACTOR), max(1, height // BLUR_FACTOR)
    for _ in range(2):
        image = image.scale_simple(small_w, small_h, GdkPixbuf.InterpType.BILINEAR)
        image = image.scale_simple(width, height, GdkPixbuf.InterpType.BILINEAR)
    if image.get_has_alpha():
        image = image.composite_color_simple(width, height, GdkPixbuf.InterpType.NEAREST, 255, 1, 0, 0)
    dark = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, True, 8, width, height)
    dark.fill(DARKEN)  # 0x000000 com alfa DARKEN
    with_alpha = image.add_alpha(False, 0, 0, 0)
    dark.composite(with_alpha, 0, 0, width, height, 0, 0, 1, 1, GdkPixbuf.InterpType.NEAREST, 255)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.{os.getpid()}")
    with_alpha.savev(str(tmp), "png", [], [])
    os.replace(tmp, path)
    prune(path.parent, keep=keep, current=path)
    return path


def prune(folder, keep, current=None):
    """Fica com os `keep` fundos mais novos (e sempre o atual)."""
    files = sorted(folder.glob("lock-*.png"), key=lambda p: p.stat().st_mtime, reverse=True)
    for old in files[keep:]:
        if old != current:
            try:
                old.unlink()
            except OSError:
                pass


# --- O estado do sistema no canto (fase 3): bateria, rede e layout do teclado, sem GTK ---

POWER_SUPPLY = Path("/sys/class/power_supply")
SYS_NET = Path("/sys/class/net")
PROC_WIRELESS = Path("/proc/net/wireless")
# Os mesmos ícones da barra (waybar-battery e waybar-network)
BATTERY_ICONS = ("", "", "", "", "")
BATTERY_CHARGING = ""
BATTERY_PLUGGED = ""
WIFI_ICONS = {1: "\U000f091f", 2: "\U000f0922", 3: "\U000f0925", 4: "\U000f0928"}
ETHERNET_ICON = "\U000f0200"
OFFLINE_ICON = "\U000f0318"


def _read(path):
    try:
        return Path(path).read_text(encoding="utf-8", errors="replace").strip()
    except OSError:
        return ""


def battery(root=POWER_SUPPLY):
    """(nível, "charging" | "plugged" | "discharging") da primeira bateria, ou None sem bateria (o
    ícone some). Lê o /sys direto: sem o upower, e sem processo a mais na tela bloqueada."""
    root = Path(root)
    try:
        supplies = sorted(root.iterdir())
    except OSError:
        return None
    ac = any(_read(d / "online") == "1" for d in supplies if _read(d / "type") == "Mains")
    for supply in supplies:
        if _read(supply / "type") != "Battery" or _read(supply / "scope") == "Device":
            continue  # o mouse e o fone sem fio também aparecem aqui, com scope Device
        level = _read(supply / "capacity")
        if not level.isdigit():
            continue
        status = _read(supply / "status")
        if status == "Charging":
            state = "charging"
        elif ac or status in ("Full", "Not charging"):
            state = "plugged"
        else:
            state = "discharging"
        return min(int(level), 100), state
    return None


def battery_icon(level, state):
    if state == "charging":
        return BATTERY_CHARGING
    if state == "plugged":
        return BATTERY_PLUGGED
    return BATTERY_ICONS[4 if level >= 100 else level // 20]


def wifi_strength(name, wireless=PROC_WIRELESS):
    """A qualidade do link em %, do /proc/net/wireless (a escala é 0 a 70), ou None."""
    for line in _read(wireless).splitlines()[2:]:
        iface, _, rest = line.partition(":")
        fields = rest.split()
        if iface.strip() == name and len(fields) >= 2:
            try:
                return max(0, min(100, round(float(fields[1].rstrip(".")) * 100 / 70)))
            except ValueError:
                return None
    return None


def network(sys_net=SYS_NET, wireless=PROC_WIRELESS):
    """("wifi", força) | ("ethernet", None) | ("offline", None). Só placas de verdade (com device) e
    ligadas; o Wi-Fi ganha do cabo se os dois estiverem no ar, como o ícone da barra."""
    sys_net = Path(sys_net)
    try:
        names = sorted(p.name for p in sys_net.iterdir())
    except OSError:
        return "offline", None
    wired = False
    for name in names:
        iface = sys_net / name
        if name == "lo" or not (iface / "device").exists() or _read(iface / "operstate") != "up":
            continue
        if (iface / "wireless").exists() or (iface / "phy80211").exists():
            strength = wifi_strength(name, wireless)
            return "wifi", 0 if strength is None else strength
        wired = True
    return ("ethernet", None) if wired else ("offline", None)


def network_icon(kind, strength):
    if kind == "wifi":
        bars = 1 if strength < 25 else 2 if strength < 50 else 3 if strength < 75 else 4
        return WIFI_ICONS[bars]
    return ETHERNET_ICON if kind == "ethernet" else OFFLINE_ICON


def keyboard_layout(inputs):
    """O nome do layout ativo do primeiro teclado, do `swaymsg -r -t get_inputs` já lido (lista), e se há
    mais de um layout para trocar. ("", False) sem teclado."""
    for device in inputs or []:
        if device.get("type") == "keyboard" and device.get("xkb_active_layout_name"):
            return device["xkb_active_layout_name"], len(device.get("xkb_layout_names") or []) > 1
    return "", False


# --- A mídia (fase 3): só capas locais na tela bloqueada ---

def local_art(url, cache=None):
    """O arquivo da capa, só se já estiver no disco: um file:// ou a do cache do popup de mídia. Nunca
    baixa nada: um decodificador de imagem com defeito, alimentado pela internet, derrubaria o processo
    que segura a sessão. None para data: (tratado à parte, sem rede) e para o resto."""
    import waybar_media as wm

    path = wm.art_path(url, cache)
    if path is None:
        return None
    try:
        if path.is_file() and path.stat().st_size <= wm.ART_MAX_BYTES:
            return path
    except OSError:
        pass
    return None


def art_bytes(url, cache=None):
    """(bytes, formato) da capa para o PixbufLoader.new_with_type, ou None. Só capa local ou data:, e só
    PNG, JPEG, GIF ou WebP pelos primeiros bytes (waybar_media.image_type): os outros carregadores do
    GdkPixbuf (ICO, TGA, XPM...) têm histórico de falhas, e o decode roda no processo do bloqueio."""
    import waybar_media as wm

    if not url:
        return None
    if url.startswith("data:"):
        data = wm.data_url_bytes(url)
    else:
        path = local_art(url, cache)
        data = wm.read_limited(path) if path is not None else None
    kind = wm.image_type(data) if data else None
    return (data, kind) if kind else None
