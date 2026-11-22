"""As traduções dos textos do desktop, pelo gettext (domínio sway-desktop).

O texto no código é o inglês; as outras línguas vêm de ~/.local/share/locale, ao lado desta pasta.
A língua é a do locale do config ou, sem ele, a do ambiente (desktop_config.message_languages).
Uso: from sway_i18n import _, ngettext, pgettext, decimal
"""
import gettext
from pathlib import Path

import desktop_config as dc

DOMAIN = "sway-desktop"
LOCALEDIR = Path(__file__).resolve().parents[1] / "share/locale"


# Instalado por pacote (swaydesk-scripts), o módulo fica em /usr/libexec/swaydesk, e as traduções em
# /usr/share/locale (swaydesk-common)
SYSTEM_LOCALEDIR = Path("/usr/share/locale")


def translation(languages):
    # Sem nenhuma língua, o gettext leria o ambiente: o "en" garante o texto do código
    for localedir in (LOCALEDIR, SYSTEM_LOCALEDIR):
        found = gettext.translation(DOMAIN, localedir, languages=languages or ["en"], fallback=True)
        if type(found) is not gettext.NullTranslations:  # achou o .mo nesta pasta
            return found
    return gettext.NullTranslations()


_languages = dc.message_languages(dc.load_config(dc.path())[0])
_translation = translation(_languages)
_ = _translation.gettext
ngettext = _translation.ngettext
pgettext = _translation.pgettext


def decimal(value, digits=1):
    """O número com o separador decimal da língua: 12.5 em inglês, 12,5 em português."""
    # TRANSLATORS: the decimal separator of your language
    return f"{value:.{digits}f}".replace(".", pgettext("decimal separator", "."))


# Os nomes dos layouts de teclado (o xkb_active_layout_name do Sway, em inglês) vêm traduzidos do próprio
# xkeyboard-config (xkb-data no Debian): xkeyboard-config-2 nas versões novas, xkeyboard-config nas outras. No
# Ubuntu e no Mint, vêm nos language-pack-<língua>-base, em /usr/share/locale-langpack, que o gettext do Python
# não procura sozinho
XKB_DOMAINS = ("xkeyboard-config-2", "xkeyboard-config")
XKB_LOCALEDIRS = (Path("/usr/local/share/locale"), Path("/usr/share/locale-langpack"), SYSTEM_LOCALEDIR)


def xkb_translation(languages, localedirs=XKB_LOCALEDIRS):
    for language in languages or ["en"]:
        for localedir in localedirs:
            for domain in XKB_DOMAINS:
                found = gettext.translation(domain, localedir, languages=[language], fallback=True)
                if type(found) is not gettext.NullTranslations:
                    return found
    return gettext.NullTranslations()


_xkb = None


def layout_name(name):
    """O nome de um layout de teclado na língua da sessão ("Portuguese (Brazil)" → "Português (Brasil)").
    Sem a tradução instalada, o nome em inglês, como veio; vazio fica vazio (o msgid "" é o cabeçalho)."""
    global _xkb
    if not name:
        return ""
    if _xkb is None:
        _xkb = xkb_translation(_languages)
    return _xkb.gettext(name)
