#!/usr/bin/env python3
"""O PKGBUILD do OBS: o packaging/arch/PKGBUILD com a versão do snapshot e a fonte no tarball do
make-orig.sh (o mesmo .orig.tar.xz do Debian, com os crates em rust/vendor: o OBS compila sem rede).
Troca só as linhas de versão e de fonte, e falha se alguma delas mudou de forma no PKGBUILD.
uso: pkgbuild.py PKGBUILD VERSÃO_ARCH TARBALL > PKGBUILD_DO_OBS"""
import hashlib
import sys
from pathlib import Path

# As linhas do packaging/arch/PKGBUILD que mudam, como estão lá
LINES = {
    "pkgver": "pkgver=0.1.0",
    "pkgrel": "pkgrel=1",
    "source": 'source=("$pkgbase-$pkgver.tar.gz::https://github.com/nielsonrolim/sway-desktop/archive/refs/tags/v$pkgver.tar.gz")',
    "sha256sums": "sha256sums=('SKIP')  # na publicação: o sha256 do tarball da tag",
    "_src": "_src=sway-desktop-$pkgver",
}


def render(pkgbuild, version, tarball):
    """O texto do PKGBUILD do OBS. tarball: o .orig.tar.xz, com a pasta swaydesk-<versão upstream>/."""
    name = Path(tarball).name
    if not (name.startswith("swaydesk_") and name.endswith(".orig.tar.xz")):
        raise ValueError(f"not a make-orig tarball: {name}")
    upstream = name[len("swaydesk_"):-len(".orig.tar.xz")]
    for char in "'\"$`\\ ":
        if char in version or char in upstream:
            raise ValueError(f"unexpected character in the version: {version!r} {upstream!r}")
    sha256 = hashlib.sha256(Path(tarball).read_bytes()).hexdigest()
    new = {
        "pkgver": f"pkgver={version}",
        "pkgrel": "pkgrel=1",
        "source": f"source=('{name}')",
        "sha256sums": f"sha256sums=('{sha256}')",
        "_src": f"_src=swaydesk-{upstream}",
    }
    lines = pkgbuild.split("\n")
    for key, old in LINES.items():
        found = [i for i, line in enumerate(lines) if line == old]
        if len(found) != 1:
            raise ValueError(f"PKGBUILD: expected the line {old!r} once, found {len(found)}")
        lines[found[0]] = new[key]
    return "\n".join(lines)


def main(argv):
    if len(argv) != 4:
        sys.exit(__doc__.strip().splitlines()[-1])
    try:
        sys.stdout.write(render(Path(argv[1]).read_text(encoding="utf-8"), argv[2], argv[3]))
    except ValueError as error:
        sys.exit(f"pkgbuild.py: {error}")


if __name__ == "__main__":
    main(sys.argv)
