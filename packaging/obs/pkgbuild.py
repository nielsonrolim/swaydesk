#!/usr/bin/env python3
"""O PKGBUILD do OBS: o packaging/arch/PKGBUILD com a versão do snapshot e a fonte no tarball do
make-orig.sh (o mesmo .orig.tar.xz do Debian, com os crates em rust/vendor: o OBS compila sem rede).
Troca só as linhas de versão e de fonte, e falha se alguma delas mudou de forma no PKGBUILD.
uso: pkgbuild.py PKGBUILD VERSÃO_ARCH TARBALL [PKGREL] > PKGBUILD_DO_OBS   (PKGREL: a revisão, padrão 1)"""
import hashlib
import re
import sys
from pathlib import Path

# As linhas do packaging/arch/PKGBUILD que mudam, como estão lá. O pkgver é o da próxima versão, que sobe a
# cada lançamento: vale qualquer X.Y.Z, ou X.Y.Z.W numa correção (release/X.Y.Z), e só eles
PKGVER = re.compile(r"pkgver=[0-9]+\.[0-9]+\.[0-9]+(\.[0-9]+)?")
LINES = {
    "pkgver": PKGVER,
    "pkgrel": "pkgrel=1",
    "source": 'source=("$pkgbase-$pkgver.tar.gz::https://github.com/nielsonrolim/sway-desktop/archive/refs/tags/v$pkgver.tar.gz")',
    "sha256sums": "sha256sums=('SKIP')  # na publicação: o sha256 do tarball da tag",
    "_src": "_src=sway-desktop-$pkgver",
}


def render(pkgbuild, version, tarball, pkgrel="1"):
    """O texto do PKGBUILD do OBS. tarball: o .orig.tar.xz, com a pasta swaydesk-<versão upstream>/. pkgrel: a
    revisão do empacotamento (o N da tag vX.Y.Z-N; 1 no lançamento e nos snapshots)."""
    name = Path(tarball).name
    if not (name.startswith("swaydesk_") and name.endswith(".orig.tar.xz")):
        raise ValueError(f"not a make-orig tarball: {name}")
    upstream = name[len("swaydesk_"):-len(".orig.tar.xz")]
    for char in "'\"$`\\ ":
        if char in version or char in upstream:
            raise ValueError(f"unexpected character in the version: {version!r} {upstream!r}")
    if not re.fullmatch(r"[1-9][0-9]*", str(pkgrel)):
        raise ValueError(f"unexpected pkgrel: {pkgrel!r}")
    sha256 = hashlib.sha256(Path(tarball).read_bytes()).hexdigest()
    new = {
        "pkgver": f"pkgver={version}",
        "pkgrel": f"pkgrel={pkgrel}",
        "source": f"source=('{name}')",
        "sha256sums": f"sha256sums=('{sha256}')",
        "_src": f"_src=swaydesk-{upstream}",
    }
    lines = pkgbuild.split("\n")
    for key, old in LINES.items():
        found = [i for i, line in enumerate(lines)
                 if (old.fullmatch(line) if isinstance(old, re.Pattern) else line == old)]
        if len(found) != 1:
            shown = old.pattern if isinstance(old, re.Pattern) else old
            raise ValueError(f"PKGBUILD: expected the line {shown!r} once, found {len(found)}")
        lines[found[0]] = new[key]
    return "\n".join(lines)


def main(argv):
    if len(argv) not in (4, 5):
        sys.exit(__doc__.strip().splitlines()[-1])
    try:
        sys.stdout.write(render(Path(argv[1]).read_text(encoding="utf-8"), argv[2], argv[3], *argv[4:]))
    except ValueError as error:
        sys.exit(f"pkgbuild.py: {error}")


if __name__ == "__main__":
    main(sys.argv)
