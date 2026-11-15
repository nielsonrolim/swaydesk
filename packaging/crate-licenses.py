#!/usr/bin/env python3
"""As licenças dos crates de terceiros que entram nos binários.

  crate-licenses.py debian VENDOR
      Reescreve, no debian/copyright, um parágrafo Files por crate de VENDOR (a pasta do `cargo vendor
      --versioned-dirs`, a mesma do packaging/make-orig.sh), com os autores e a licença do Cargo.toml de
      cada um. Rode depois de toda mudança no rust/Cargo.lock; o tests/test_packaging.py confere que a lista
      bate com o Cargo.lock. Os textos das licenças (os parágrafos License) são mantidos à mão.

  crate-licenses.py arch PACOTE DESTINO
      No PKGBUILD, dentro de rust/ e depois do `cargo fetch`: copia para DESTINO/crates/<crate>-<versão>/ os
      arquivos de licença dos crates que os binários de PACOTE ligam (pelo `cargo metadata`), e escreve em
      DESTINO/crates/CRATES a lista com a licença de cada um.
"""
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COPYRIGHT = ROOT / "debian/copyright"
COMPONENTS = ROOT / "packaging/components.list"
VENDOR_PREFIX = "rust/vendor/"
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|UNLICENSE|NOTICE|COPYRIGHT)", re.I)
COPYRIGHT_LINE = re.compile(r"^\W*(Copyright\b.*\d{4}.*)$", re.I)


def dep5_license(spdx):
    """A expressão SPDX do Cargo.toml no formato do debian/copyright: "or" e "and" em minúsculas, o "and"
    entre grupos com "or" depois de uma vírgula, e "Apache-2.0 with LLVM exception"."""
    spdx = " OR ".join(part.strip() for part in spdx.split("/"))  # a forma antiga "MIT/Apache-2.0"
    depth, parts, start = 0, [], 0
    for match in re.finditer(r"[()]|\bAND\b", spdx):
        token = match.group()
        depth += {"(": 1, ")": -1}.get(token, 0)
        if token == "AND" and depth == 0:
            parts.append(spdx[start:match.start()])
            start = match.end()
    parts.append(spdx[start:])
    converted = []
    for part in parts:
        part = part.strip()
        if part.startswith("(") and part.endswith(")"):
            part = part[1:-1].strip()
        if "(" in part or ")" in part:
            raise ValueError(f"expressão de licença não suportada: {spdx}")
        part = re.sub(r"\s+WITH\s+(\S+)-exception\b", r" with \1 exception", part)
        converted.append(re.sub(r"\s+(OR|AND)\s+", lambda m: f" {m.group(1).lower()} ", part))
    joiner = ", and " if len(converted) > 1 and any(" or " in part for part in converted) else " and "
    return joiner.join(converted)


def crate_copyright(folder, package):
    """Os autores do Cargo.toml; sem eles, as linhas Copyright dos arquivos de licença."""
    authors = package.get("authors") or []
    if authors:
        return authors
    found = []
    for path in sorted(folder.iterdir()):
        if path.is_file() and LICENSE_FILE.match(path.name):
            for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
                match = COPYRIGHT_LINE.match(line)
                if not match:
                    continue
                holder = re.sub(r"^Copyright\s*|\(c\)\s*|©\s*|\s*All rights reserved\.?", "", match.group(1),
                                flags=re.I).strip(" :,.")
                holder = re.sub(r"^(\d[\d\s-]*\d),\s*", r"\1 ", holder)  # "2016, Fulano" -> "2016 Fulano"
                if not re.search(r"[^\W\d_]", holder):  # só os anos, sem quem
                    holder = f"{holder} the {package['name']} authors"
                if holder not in found:
                    found.append(holder)
    return found or [f"the {package['name']} authors"]


def vendor_paragraphs(vendor):
    import tomllib
    paragraphs = []
    for folder in sorted(Path(vendor).iterdir(), key=lambda path: path.name):
        manifest = folder / "Cargo.toml"
        if not manifest.is_file():
            continue
        with manifest.open("rb") as handle:
            package = tomllib.load(handle)["package"]
        if not package.get("license"):
            raise SystemExit(f"crate-licenses: {folder.name} has no license in Cargo.toml; write its paragraph by hand")
        holders = "\n ".join(crate_copyright(folder, package))
        paragraphs.append(f"Files: {VENDOR_PREFIX}{folder.name}/*\nCopyright: {holders}\n"
                          f"License: {dep5_license(package['license'])}")
    if not paragraphs:
        raise SystemExit(f"crate-licenses: no crate in {vendor}")
    return paragraphs


def rewrite_debian(vendor):
    """Troca os parágrafos Files de rust/vendor/ pelos de VENDOR, antes do primeiro parágrafo License."""
    paragraphs = COPYRIGHT.read_text(encoding="utf-8").rstrip("\n").split("\n\n")
    kept = [p for p in paragraphs if not p.startswith(f"Files: {VENDOR_PREFIX}")]
    first_license = next(i for i, p in enumerate(kept) if p.startswith("License:"))
    kept[first_license:first_license] = vendor_paragraphs(vendor)
    COPYRIGHT.write_text("\n\n".join(kept) + "\n", encoding="utf-8")


def linked_crates(metadata, roots):
    """Os pacotes de terceiros que os crates `roots` alcançam pelas dependências normais (as de build e de
    teste não entram no binário). Devolve os pacotes do `cargo metadata`, sem os do workspace."""
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    by_name = {package["name"]: package["id"] for package in metadata["packages"]
               if package["id"] in metadata["workspace_members"]}
    pending = [by_name[name] for name in roots]
    seen = set()
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        for dep in nodes[current]["deps"]:
            if any(kind["kind"] is None for kind in dep["dep_kinds"]):
                pending.append(dep["pkg"])
    return sorted((packages[i] for i in seen if packages[i]["source"]), key=lambda p: (p["name"], p["version"]))


def package_crates(package):
    """Os crates swaydesk-<componente> do pacote; o swaydesk-common, sem binário, não está no components.list
    (como no install-tree.sh)."""
    if package == "swaydesk-common":
        return []
    for line in COMPONENTS.read_text(encoding="utf-8").splitlines():
        name, _, components = line.partition(":")
        if not line.startswith("#") and name.strip() == package:
            return [f"swaydesk-{component}" for component in components.split()]
    raise SystemExit(f"crate-licenses: unknown package: {package}")


def install_arch(package, dest):
    roots = package_crates(package)
    if not roots:
        return
    host = re.search(r"^host: (\S+)$", subprocess.run(["rustc", "-vV"], capture_output=True, text=True,
                                                      check=True).stdout, re.M).group(1)
    metadata = json.loads(subprocess.run(
        ["cargo", "metadata", "--frozen", "--format-version", "1", "--filter-platform", host],
        stdout=subprocess.PIPE, text=True, check=True).stdout)  # o stderr do cargo passa, para o log do makepkg
    crates = Path(dest) / "crates"
    lines = []
    for crate in linked_crates(metadata, roots):
        folder = Path(crate["manifest_path"]).parent
        target = crates / f"{crate['name']}-{crate['version']}"
        for path in sorted(folder.iterdir()):
            if path.is_file() and LICENSE_FILE.match(path.name):
                target.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, target / path.name)
                os.chmod(target / path.name, 0o644)
        lines.append(f"{crate['name']} {crate['version']}: {crate['license'] or crate['license_file']}")
    crates.mkdir(parents=True, exist_ok=True)
    (crates / "CRATES").write_text(
        "# The third-party crates linked into the binaries of this package, with their license. The SPDX\n"
        "# texts are in /usr/share/licenses/spdx; the files of each crate, when it ships any, are next to this.\n"
        + "\n".join(lines) + "\n", encoding="utf-8")


def main(argv):
    if len(argv) == 3 and argv[1] == "debian":
        rewrite_debian(argv[2])
    elif len(argv) == 4 and argv[1] == "arch":
        install_arch(argv[2], argv[3])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
