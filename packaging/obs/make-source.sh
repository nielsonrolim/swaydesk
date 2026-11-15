#!/usr/bin/env bash
# Os pacotes-fonte do OBS a partir do commit atual (HEAD), um por pasta em SAÍDA (uma pasta por pacote do
# OBS). Em SAÍDA/swaydesk:
#   - swaydesk_<versão>.orig.tar.xz: o make-orig.sh, com os crates em rust/vendor (o OBS compila sem rede);
#   - swaydesk_<versão>-1.dsc e .debian.tar.xz: o debian/ com a entrada do snapshot no changelog
#     (packaging/version.sh); servem ao Debian 13 e ao Ubuntu 26.04;
#   - PKGBUILD e os .install: o packaging/arch/PKGBUILD com a mesma fonte (packaging/obs/pkgbuild.py).
# Em SAÍDA/swaydesk-archive-keyring, o .dsc e o .tar.xz do packaging/obs/keyring (Debian e Ubuntu).
# Precisa de git, cargo (o cargo vendor baixa os crates), dpkg-source, xz e python3: rode no contêiner,
# como no packaging/obs/README.md. Depois, o push.sh envia a pasta ao OBS.
# uso: packaging/obs/make-source.sh SAÍDA
set -euo pipefail
root=$(cd "$(dirname "$(readlink -f "$0")")/../.." && pwd)
base=$(mkdir -p "${1:?uso: make-source.sh SAÍDA}" && cd "$1" && pwd)
[[ -z $(find "$base" -mindepth 1 -maxdepth 1 -print -quit) ]] || { echo "make-source: $base is not empty" >&2; exit 1; }
out=$base/swaydesk keyring=$base/swaydesk-archive-keyring
mkdir "$out" "$keyring"
upstream=$("$root/packaging/version.sh")
arch=$("$root/packaging/version.sh" arch)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

"$root/packaging/make-orig.sh" "$work" >/dev/null
tarball=$work/swaydesk_$upstream.orig.tar.xz

# O .dsc: a árvore do tarball com o debian/ do commit (o git archive, como o make-orig: nada da árvore de
# trabalho entra)
tar -xJf "$tarball" -C "$work"
git -C "$root" archive --format=tar HEAD debian | tar -x -C "$work/swaydesk-$upstream"
"$root/packaging/version.sh" changelog "$work/swaydesk-$upstream/debian/changelog"
(cd "$work" && dpkg-source --build "swaydesk-$upstream" >/dev/null)

git -C "$root" archive --format=tar HEAD packaging/arch | tar -x -C "$work"
python3 "$root/packaging/obs/pkgbuild.py" "$work/packaging/arch/PKGBUILD" "$arch" "$tarball" > "$out/PKGBUILD"
cp "$work"/packaging/arch/*.install "$out/"
mv "$tarball" "$work"/swaydesk_"$upstream"-1.dsc "$work"/swaydesk_"$upstream"-1.debian.tar.xz "$out/"

git -C "$root" archive --format=tar HEAD packaging/obs/keyring | tar -x -C "$work"
(cd "$work/packaging/obs" && dpkg-source --build keyring >/dev/null && mv swaydesk-archive-keyring_* "$keyring/")
(cd "$base" && ls -- */)
