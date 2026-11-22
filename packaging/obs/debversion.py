#!/usr/bin/env python3
"""Compara versões do Debian como o `dpkg --compare-versions`, para o push.sh rodar sem o dpkg (num PC Arch).

uso: debversion.py A OP B   (OP: lt le eq ne ge gt; sai com 0 se a relação vale, 1 se não, 2 num erro)

O algoritmo é o do dpkg (lib/dpkg/version.c): época numérica, depois upstream e revisão comparados em trechos
alternados de não-dígitos (letras antes dos outros caracteres, o ~ antes de tudo, até do fim) e de números.
Mais estrito que o dpkg, de propósito: um upstream que não começa por dígito (no dpkg, só um aviso) ou uma
versão vazia é um erro, e o push.sh para em vez de enviar.
"""
import re
import sys

_VERSION = re.compile(r"^(?:([0-9]+):)?([0-9][A-Za-z0-9.+~:-]*?)(?:-([A-Za-z0-9.+~]+))?$")


def parse(text):
    """(época, upstream, revisão) de uma versão; ValueError se não for válida."""
    match = _VERSION.match(text.strip())
    if not match or ("-" in match.group(2) and match.group(3) is None) or (":" in match.group(2) and not match.group(1)):
        raise ValueError(f"invalid version: {text!r}")
    return int(match.group(1) or 0), match.group(2), match.group(3) or ""


DIGITS = "0123456789"


def _order(char):
    if char == "~":
        return -1
    if char.isascii() and char.isalpha():
        return ord(char)
    return ord(char) + 256


def _compare_part(a, b):
    i = j = 0
    while i < len(a) or j < len(b):
        # Os não-dígitos, caractere a caractere; o fim vale 0 (o ~ fica abaixo dele)
        while (i < len(a) and not a[i] in DIGITS) or (j < len(b) and not b[j] in DIGITS):
            ca = _order(a[i]) if i < len(a) and not a[i] in DIGITS else 0
            cb = _order(b[j]) if j < len(b) and not b[j] in DIGITS else 0
            if ca != cb:
                return ca - cb
            i, j = i + 1, j + 1
        # Os dígitos, como números
        si = i
        while i < len(a) and a[i] in DIGITS:
            i += 1
        sj = j
        while j < len(b) and b[j] in DIGITS:
            j += 1
        # Sem int(): um número enorme passaria do limite de dígitos do Python. Sem os zeros à esquerda, o mais
        # comprido é o maior; do mesmo tamanho, a ordem do texto
        na, nb = a[si:i].lstrip("0"), b[sj:j].lstrip("0")
        if na != nb:
            return (len(na) > len(nb)) - (len(na) < len(nb)) or (na > nb) - (na < nb)
    return 0


def compare(a, b):
    """Negativo, zero ou positivo, como o dpkg."""
    ea, ua, ra = parse(a)
    eb, ub, rb = parse(b)
    return (ea - eb) or _compare_part(ua, ub) or _compare_part(ra, rb)


OPS = {"lt": lambda c: c < 0, "le": lambda c: c <= 0, "eq": lambda c: c == 0,
       "ne": lambda c: c != 0, "ge": lambda c: c >= 0, "gt": lambda c: c > 0}


def main(argv):
    if len(argv) != 3 or argv[1] not in OPS:
        print("usage: debversion.py A lt|le|eq|ne|ge|gt B", file=sys.stderr)
        return 2
    try:
        return 0 if OPS[argv[1]](compare(argv[0], argv[2])) else 1
    except ValueError as err:
        print(f"debversion: {err}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
