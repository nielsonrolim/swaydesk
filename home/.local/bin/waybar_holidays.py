"""Os feriados do calendário, sem GTK: a python3-holidays para a versão em Python do popup do
calendário (waybar-calendar) e, por linha de comando, para a versão em Rust.

uso: waybar_holidays.py ANO    os feriados do ano e dos vizinhos, em JSON:
     {"listed": bool, "note": texto ou null, "holidays": [["2026-12-25", "Natal", false], ...]}
O país e a língua vêm da config (holidays e locale), como no popup.
"""
import json
import sys

from sway_i18n import _


class OldLibrary(Exception):
    """A python3-holidays sem o country_holidays, como a 0.10 do Mint 22."""


def library_holidays(lib, country, years, language):
    """{data: (nome, facultativo)} pela python3-holidays (0.71 no Debian 13, 0.90 no Ubuntu 26.04), com
    os nomes na língua da interface e os facultativos separados. Levanta LookupError se ela não conhece
    o país, e OldLibrary com a versão antiga: a 0.10 tem os feriados do Brasil desatualizados (sem o
    20 de novembro, com a Páscoa e a Quarta de Cinzas como feriados), e dados errados são piores que nada."""
    if not hasattr(lib, "country_holidays"):
        raise OldLibrary(getattr(lib, "__version__", "?"))

    def fetch(**extra):
        try:
            return lib.country_holidays(country, years=years, language=language, **extra)
        except NotImplementedError as err:
            raise LookupError(country) from err
        except (TypeError, ValueError):  # uma língua que o país não tem: os nomes na língua dele
            return lib.country_holidays(country, years=years, **extra)

    shown = {d: (name, False) for d, name in fetch().items()}
    try:
        optional = fetch(categories=("optional",))
    except (TypeError, ValueError, LookupError):
        optional = {}
    for d, name in optional.items():
        shown.setdefault(d, (name, True))
    return shown


def shown(country, year, language, explicit=True):
    """Os feriados do ano e dos vizinhos (dezembro e janeiro aparecem na grade), se houver país.
    Devolve (feriados, aviso, listar): listar diz se a caixa dos feriados aparece, e o aviso, por que
    ela está vazia. Com a python3-holidays antiga, a caixa some: a versão do sistema não é escolha de
    quem usa, e o aviso ficaria lá até a próxima versão da distro. explicit: o país veio da config. Com o
    país do locale, que ninguém escolheu, a falta da biblioteca ou um país que ela não conhece só
    esconde a caixa, sem aviso."""
    if not country:
        return {}, None, False
    try:
        import holidays as lib
    except ImportError:
        if not explicit:
            return {}, None, False  # ninguém pediu feriados: sem a biblioteca opcional, sem aviso
        return {}, _("Install python3-holidays to see the holidays"), True
    try:
        return library_holidays(lib, country, [year - 1, year, year + 1], language), None, True
    except LookupError:
        if not explicit:
            return {}, None, False
        return {}, _("No holidays known for %s") % country, True
    except OldLibrary:
        return {}, None, False


def main(argv):
    import desktop_config

    if len(argv) != 1 or not argv[0].isdigit():
        print("usage: waybar_holidays.py YEAR", file=sys.stderr)
        return 2
    config = desktop_config.load()
    languages = desktop_config.message_languages(config)
    country, explicit = desktop_config.holidays_country(config)
    found, note, listed = shown(country, int(argv[0]), languages[0] if languages else "en_US", explicit)
    days = [[d.isoformat(), name, optional] for d, (name, optional) in sorted(found.items())]
    print(json.dumps({"listed": listed, "note": note, "holidays": days}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
