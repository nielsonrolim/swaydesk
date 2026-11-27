#!/usr/bin/env python3
"""Os pacotes de terceiros que levamos no OBS (packaging/obs/upstream.list) contra a última versão do upstream: rodar
a cada RC e lançamento. Uma linha por pacote: "<pasta em packaging/obs> github:<dono>/<repositório>".

Saída, por pacote: "up to date", "NEWER <versão>" ou "check failed: <motivo>". Sai com 0 com tudo em dia, 1 com algum
mais novo e 2 quando alguma conferência falhou (sem rede, resposta estranha): nunca em silêncio. Um NEWER não trava o
lançamento sozinho: é relatado, e o usuário decide se atualiza.

A resposta da rede não é confiável: lida com um teto de tamanho, só como JSON, e a versão só vale se casar com a
expressão abaixo; nada dela é executado nem impresso fora disso.

SWAYDESK_OBS_DIR e SWAYDESK_GITHUB_API (por exemplo um file:// local) trocam a pasta e a API, para os testes."""
import json
import os
import re
import sys
import urllib.request
from pathlib import Path

OBS = Path(os.environ.get("SWAYDESK_OBS_DIR") or Path(__file__).resolve().parent / "obs")
API = os.environ.get("SWAYDESK_GITHUB_API") or "https://api.github.com"
VERSION = re.compile(r"\A[0-9]+(?:\.[0-9]+)*\Z")
REPO = re.compile(r"\A[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\Z")
MAX_ANSWER = 1 << 20


class Failed(Exception):
    pass


def ours(name):
    """A versão upstream do pacote: a do debian/changelog, sem a revisão (6.5-2 → 6.5)."""
    try:
        first = (OBS / name / "debian/changelog").read_text(encoding="utf-8").split("\n", 1)[0]
    except OSError as err:
        raise Failed(f"no debian/changelog ({err.strerror})") from None
    found = re.match(rf"{re.escape(name)} \(([^)]+)\)", first)
    version = found.group(1).rsplit("-", 1)[0].split(":")[-1] if found else ""
    if not VERSION.match(version):
        raise Failed("no version in debian/changelog")
    return version


def latest_github(repo):
    if not REPO.match(repo):
        raise Failed("bad repository name")
    request = urllib.request.Request(f"{API}/repos/{repo}/releases/latest",
                                     headers={"Accept": "application/vnd.github+json",
                                              "User-Agent": "swaydesk-check-upstream"})
    try:
        with urllib.request.urlopen(request, timeout=30) as answer:
            body = answer.read(MAX_ANSWER + 1)
    except (OSError, ValueError) as err:
        raise Failed(f"no answer ({type(err).__name__})") from None
    if len(body) > MAX_ANSWER:
        raise Failed("answer too big")
    try:
        data = json.loads(body)
    except ValueError:
        raise Failed("answer is not JSON") from None
    tag = data.get("tag_name") if isinstance(data, dict) else None
    version = tag[1:] if isinstance(tag, str) and tag.startswith("v") else tag
    if not isinstance(version, str) or not VERSION.match(version):
        raise Failed("no release version in the answer")
    return version


SOURCES = {"github": latest_github}


def key(version):
    """Para comparar: os números, sem os zeros do fim (6.5.0 é a 6.5)."""
    parts = [int(part) for part in version.split(".")]
    while len(parts) > 1 and parts[-1] == 0:
        parts.pop()
    return tuple(parts)


def check(name, source):
    kind, _, where = source.partition(":")
    if kind not in SOURCES:
        raise Failed(f"unknown source {kind!r}")
    have = ours(name)
    latest = SOURCES[kind](where)
    if key(latest) > key(have):
        return 1, f"NEWER {latest} (we have {have})"
    return 0, f"up to date ({have})"


def main():
    try:
        lines = (OBS / "upstream.list").read_text(encoding="utf-8").splitlines()
    except OSError as err:
        print(f"check-upstream: {OBS / 'upstream.list'}: {err.strerror}", file=sys.stderr)
        return 2
    worst = 0
    for line in lines:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        fields = line.split()
        name = fields[0]
        try:
            if len(fields) != 2:
                raise Failed("expected '<package> <source>'")
            status, text = check(name, fields[1])
        except Failed as err:
            status, text = 2, f"check failed: {err}"
        print(f"{name}: {text}")
        worst = max(worst, status)
    return worst


if __name__ == "__main__":
    sys.exit(main())
