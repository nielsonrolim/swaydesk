"""Os players de mídia pelo MPRIS, sem GTK: o que o popup de mídia lê e os comandos que manda.

As chamadas são síncronas, com prazo curto: o popup as faz fora da linha principal. O barramento é
um parâmetro, para os testes usarem um só deles.
"""
import base64
import binascii
import hashlib
import ipaddress
import os
import socket
import stat
import tempfile
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

MPRIS = "org.mpris.MediaPlayer2"
PLAYER = "org.mpris.MediaPlayer2.Player"
PATH = "/org/mpris/MediaPlayer2"
NO_TRACK = "/org/mpris/MediaPlayer2/TrackList/NoTrack"
TIMEOUT_MS = 1000
PLAYING, PAUSED = "Playing", "Paused"
ART_MAX_BYTES = 5 * 1024 * 1024
ART_KEEP = 50  # capas guardadas no cache; as mais antigas saem


@dataclass
class State:
    name: str  # o nome no barramento: org.mpris.MediaPlayer2.firefox.instance_1_42
    identity: str = ""
    status: str = "Stopped"
    title: str = ""
    artist: str = ""
    album: str = ""
    art_url: str = ""
    length: int = 0  # microssegundos; 0: desconhecida
    track_id: str = ""
    position: int = 0
    can_next: bool = False
    can_previous: bool = False
    can_seek: bool = False
    can_pause: bool = False
    can_play: bool = False
    owner: str = ""  # o nome único no barramento, de quem vêm os sinais

    @property
    def active(self):
        """Há mídia: tocando ou pausada. Parado ou sem nada, o componente some."""
        return self.status in (PLAYING, PAUSED)

    @property
    def label(self):
        """O nome para a escolha do player."""
        return self.identity or self.name.removeprefix(MPRIS + ".").split(".")[0]


def unpack(value):
    return value.unpack() if isinstance(value, GLib.Variant) else value


def text(value):
    """Um campo de texto do player, que pode mandar qualquer tipo: o que não é texto vira vazio."""
    return value if isinstance(value, str) else ""


def from_props(name, identity, props):
    """O estado a partir das propriedades da interface Player (já desempacotadas). Cada campo é
    conferido aqui: um player com tipos errados não pode quebrar a janela depois."""
    meta = props.get("Metadata")
    if not isinstance(meta, dict):
        meta = {}
    artists = meta.get("xesam:artist")
    if isinstance(artists, str):
        artists = [artists]
    elif not isinstance(artists, (list, tuple)):
        artists = []

    def number(value):
        if isinstance(value, bool):
            return 0
        try:
            return max(0, int(value or 0))
        except (TypeError, ValueError, OverflowError):
            return 0

    status = text(props.get("PlaybackStatus")) or "Stopped"
    return State(
        name=name, identity=text(identity), status=status,
        title=text(meta.get("xesam:title")), artist=", ".join(a for a in artists if isinstance(a, str) and a),
        album=text(meta.get("xesam:album")), art_url=text(meta.get("mpris:artUrl")),
        length=number(meta.get("mpris:length")), track_id=text(meta.get("mpris:trackid")),
        position=number(props.get("Position")), can_next=props.get("CanGoNext") is True,
        can_previous=props.get("CanGoPrevious") is True, can_seek=props.get("CanSeek") is True,
        can_pause=props.get("CanPause") is True, can_play=props.get("CanPlay") is True)


def pick(states, preferred=None):
    """O player a mostrar: o escolhido, se ainda tiver mídia; senão o primeiro tocando e, sem nenhum,
    o primeiro pausado. A mesma regra do ícone da barra (waybar-media)."""
    active = [s for s in states if s.active]
    for state in active:
        if state.name == preferred:
            return state
    return next((s for s in active if s.status == PLAYING), active[0] if active else None)


def fmt_time(us):
    """Microssegundos -> 1:05 ou 1:02:03."""
    seconds = max(0, int(us or 0) // 1_000_000)
    hours, rest = divmod(seconds, 3600)
    minutes, seconds = divmod(rest, 60)
    return f"{hours}:{minutes:02d}:{seconds:02d}" if hours else f"{minutes}:{seconds:02d}"


# --- barramento ---

def call(bus, name, iface, method, params=None, reply=None):
    result = bus.call_sync(name, PATH, iface, method, params, reply, Gio.DBusCallFlags.NONE, TIMEOUT_MS, None)
    return result.unpack() if result is not None else None


def player_names(bus):
    names = bus.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "ListNames",
                          None, GLib.VariantType("(as)"), Gio.DBusCallFlags.NONE, TIMEOUT_MS, None).unpack()[0]
    return sorted(n for n in names if n.startswith(MPRIS + "."))


def name_owner(bus, name):
    """O nome único (":1.42") de quem tem o nome: os sinais chegam com ele."""
    return bus.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetNameOwner",
                         GLib.Variant("(s)", (name,)), GLib.VariantType("(s)"), Gio.DBusCallFlags.NONE,
                         TIMEOUT_MS, None).unpack()[0]


def read_state(bus, name, identity=None, owner=None):
    """O estado de um player; None se ele não respondeu (fechou no meio, travou) ou mandou valores
    que não se leem. identity e owner: os já conhecidos, para não perguntar de novo."""
    try:
        props = call(bus, name, "org.freedesktop.DBus.Properties", "GetAll", GLib.Variant("(s)", (PLAYER,)))[0]
        if not identity:
            try:
                identity = call(bus, name, "org.freedesktop.DBus.Properties", "Get",
                                GLib.Variant("(ss)", (MPRIS, "Identity")))[0]
            except GLib.Error:
                identity = ""
        state = from_props(name, identity, {k: unpack(v) for k, v in props.items()})
    except Exception:  # noqa: BLE001 - um player com dados ruins não pode derrubar quem lê
        return None
    if not owner:
        try:
            owner = name_owner(bus, name)
        except GLib.Error:  # só serve para achar o player pelos avisos: sem ele, a releitura é geral
            owner = ""
    state.owner = owner
    return state


def read_all(bus, identities=None, owners=None):
    """Todos os players, na ordem dos nomes (a mesma do ícone da barra); os que não respondem ficam
    de fora. identities e owners: {nome: valor} já conhecidos."""
    identities, owners = identities or {}, owners or {}
    try:
        names = player_names(bus)
    except GLib.Error:
        return []
    return [s for s in (read_state(bus, n, identities.get(n), owners.get(n)) for n in names) if s is not None]


def position(bus, name):
    try:
        return int(call(bus, name, "org.freedesktop.DBus.Properties", "Get",
                        GLib.Variant("(ss)", (PLAYER, "Position")))[0])
    except (GLib.Error, TypeError, ValueError):
        return None


def command(bus, name, method):
    """PlayPause, Next ou Previous. Devolve se o player aceitou."""
    try:
        call(bus, name, PLAYER, method)
        return True
    except GLib.Error:
        return False


def seek_to(bus, state, target):
    """Vai para target (µs). Com o id da faixa, SetPosition; sem ele (alguns players), Seek relativo."""
    target = max(0, min(int(target), state.length or int(target)))
    try:
        if state.track_id != NO_TRACK and GLib.Variant.is_object_path(state.track_id):
            call(bus, state.name, PLAYER, "SetPosition", GLib.Variant("(ox)", (state.track_id, target)))
        else:
            current = position(bus, state.name)
            if current is None:
                return False
            call(bus, state.name, PLAYER, "Seek", GLib.Variant("(x)", (target - current,)))
        return True
    except GLib.Error:
        return False


# --- capas ---

def art_cache():
    base = os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache"
    return Path(base) / "waybar-media"


def art_path(url, cache=None):
    """O arquivo da capa: o próprio, num file://; o do cache, num http(s) (pode ainda não existir).
    None: sem capa ou num esquema que não se baixa."""
    if not url:
        return None
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme == "file":
        return Path(urllib.parse.unquote(parsed.path))
    if parsed.scheme in ("http", "https"):
        return Path(cache or art_cache()) / hashlib.sha256(url.encode()).hexdigest()
    return None


def has_art(url):
    """Se a capa é de um tipo que se carrega: arquivo, rede ou embutida (data:, como no mpv-mpris 1.x)."""
    return bool(url) and (art_path(url) is not None or url.startswith("data:"))


def data_url_bytes(url):
    """Os bytes de uma capa embutida (data:...;base64,...), até o limite. O tipo declarado não vale:
    o mpv-mpris diz jpeg num PNG, e quem decide é o image_type pelos primeiros bytes."""
    header, sep, payload = url.partition(",")
    if not sep or not header.lower().endswith(";base64") or len(payload) > ART_MAX_BYTES * 4 // 3 + 4:
        return None
    try:
        return base64.b64decode(payload, validate=False)
    except (ValueError, binascii.Error):
        return None


def image_type(data):
    """O formato pelos primeiros bytes; None fora dos que se aceitam. Assim o GdkPixbuf só recebe
    PNG, JPEG, GIF ou WebP, e nunca os outros carregadores (ICO, TGA, XPM...)."""
    if data.startswith(b"\x89PNG\r\n\x1a\n"):
        return "png"
    if data.startswith(b"\xff\xd8\xff"):
        return "jpeg"
    if data[:6] in (b"GIF87a", b"GIF89a"):
        return "gif"
    if data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return "webp"
    return None


def public_url(url):
    """Só http(s) para um endereço da internet: o artUrl de um navegador vem da página, que não pode
    fazer o popup acessar a própria máquina ou a rede local.

    Risco aceito: a conexão resolve o nome de novo, e um DNS que muda de resposta entre as duas
    (rebinding) passaria. O que sobra é um GET cego: a resposta só é usada se for uma imagem de
    verdade (Content-Type e primeiros bytes), e nunca é mostrada nem devolvida a quem pediu."""
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme not in ("http", "https") or not parsed.hostname:
        return False
    try:
        infos = socket.getaddrinfo(parsed.hostname, parsed.port or 443, proto=socket.IPPROTO_TCP)
    except (OSError, UnicodeError, ValueError):
        return False
    try:
        return bool(infos) and all(ipaddress.ip_address(info[4][0].split("%")[0]).is_global for info in infos)
    except ValueError:
        return False


class PublicRedirects(urllib.request.HTTPRedirectHandler):
    """Um redirecionamento também só vai para a internet."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if not public_url(newurl):
            raise urllib.error.URLError("redirect to a private address")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def open_public(url, timeout):
    return urllib.request.build_opener(PublicRedirects).open(url, timeout=timeout)


def read_limited(path):
    """Os bytes de um arquivo comum até o limite; None se for outra coisa (FIFO, aparelho), se for
    grande demais ou não abrir. Sem bloquear num FIFO."""
    try:
        info = os.stat(path)
        if not stat.S_ISREG(info.st_mode) or info.st_size > ART_MAX_BYTES:
            return None
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
        with os.fdopen(fd, "rb") as handle:
            if not stat.S_ISREG(os.fstat(handle.fileno()).st_mode):
                return None
            data = handle.read(ART_MAX_BYTES + 1)
    except OSError:
        return None
    return data if len(data) <= ART_MAX_BYTES else None


def load_art(url, cache=None, opener=open_public, check=public_url):
    """Os bytes da capa, de um formato aceito, ou None. Fora da linha principal: lê arquivos e a rede.
    O que vem da rede fica no cache, e a capa reusada é marcada como recente."""
    if url and url.startswith("data:"):
        data = data_url_bytes(url)
        return data if data and len(data) <= ART_MAX_BYTES and image_type(data) else None
    path = art_path(url, cache)
    if path is None:
        return None
    if urllib.parse.urlparse(url).scheme == "file":
        data = read_limited(path)
        return data if data and image_type(data) else None
    data = read_limited(path)
    if data and image_type(data):
        try:
            os.utime(path)
        except OSError:
            pass
        return data
    if not check(url):
        return None
    try:
        with opener(url, timeout=5) as response:
            if not response.headers.get_content_type().startswith("image/"):
                return None
            data = response.read(ART_MAX_BYTES + 1)
    except (OSError, ValueError):  # o URLError é um OSError
        return None
    if not data or len(data) > ART_MAX_BYTES or not image_type(data):
        return None
    save_art(path, data)
    return data


def save_art(path, data):
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        fd, tmp = tempfile.mkstemp(prefix=".art.", dir=path.parent)
    except OSError:
        return
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
        os.replace(tmp, path)
    except OSError:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        return
    prune_art(path.parent)


def prune_art(cache, keep=ART_KEEP):
    """Deixa as keep mais recentes. É só faxina: outra linha apagando ao mesmo tempo, ou o cache
    inteiro sumindo, não pode impedir a capa de aparecer."""
    try:
        entries = list(Path(cache).iterdir())
    except OSError:
        return
    files = []
    for entry in entries:
        if entry.name.startswith("."):
            continue
        try:
            files.append((entry.stat().st_mtime, entry))
        except OSError:
            pass
    files.sort(reverse=True)
    for _mtime, old in files[keep:]:
        try:
            old.unlink()
        except OSError:
            pass
