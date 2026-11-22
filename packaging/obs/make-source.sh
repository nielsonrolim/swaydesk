#!/usr/bin/env bash
# Os pacotes-fonte do OBS a partir do commit atual (HEAD), um por pasta em SAÍDA (uma pasta por pacote do
# OBS). Em SAÍDA/swaydesk:
#   - swaydesk_<versão>.orig.tar.xz: o make-orig.sh, com os crates em rust/vendor (o OBS compila sem rede);
#   - swaydesk_<versão>-<revisão>.dsc e .debian.tar.xz: o debian/ com a entrada do snapshot no changelog
#     (packaging/version.sh); servem ao Debian 13 e ao Ubuntu 26.04;
#   - PKGBUILD e os .install: o packaging/arch/PKGBUILD com a mesma fonte (packaging/obs/pkgbuild.py).
# Em SAÍDA/swaydesk-archive-keyring, o .dsc e o .tar.xz do packaging/obs/keyring (Debian e Ubuntu).
# Em SAÍDA/adw-gtk-theme, o .dsc do packaging/obs/adw-gtk-theme (Debian e Ubuntu; o Arch o tem no extra), com o
# tarball do upstream como orig: baixado da URL do arquivo upstream e conferido pelo sha256.
# Precisa de git, cargo (o cargo vendor baixa os crates), dpkg-source, xz e python3: rode no contêiner,
# como no packaging/obs/README.md. Depois, o push.sh envia a pasta ao OBS.
# Numa revisão (vX.Y.Z-N, N >= 2), SWAYDESK_ORIG aponta o swaydesk_X.Y.Z.orig.tar.xz que já está no OBS (o
# da -1, baixado com osc): a mesma versão upstream tem de ter o mesmo orig, byte a byte (regra do Debian, e
# um CDN que guardou o orig antigo não serve outro com o mesmo nome). Sem ele, o orig é gerado de novo.
# uso: packaging/obs/make-source.sh SAÍDA     (SWAYDESK_ORIG=…orig.tar.xz numa revisão)
set -euo pipefail
root=$(cd "$(dirname "$(readlink -f "$0")")/../.." && pwd)
base=$(mkdir -p "${1:?uso: make-source.sh SAÍDA}" && cd "$1" && pwd)
[[ -z $(find "$base" -mindepth 1 -maxdepth 1 -print -quit) ]] || { echo "make-source: $base is not empty" >&2; exit 1; }
out=$base/swaydesk keyring=$base/swaydesk-archive-keyring adw=$base/adw-gtk-theme
mkdir "$out" "$keyring" "$adw"
upstream=$("$root/packaging/version.sh")
arch=$("$root/packaging/version.sh" arch)
deb=$("$root/packaging/version.sh" deb)
revision=$("$root/packaging/version.sh" revision)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

tarball=$work/swaydesk_$upstream.orig.tar.xz
if [[ -n ${SWAYDESK_ORIG:-} ]]; then
  [[ $(basename "$SWAYDESK_ORIG") == "swaydesk_$upstream.orig.tar.xz" && -f $SWAYDESK_ORIG ]] || {
    echo "make-source: SWAYDESK_ORIG must be swaydesk_$upstream.orig.tar.xz: $SWAYDESK_ORIG" >&2; exit 1; }
  cp -- "$SWAYDESK_ORIG" "$tarball"
elif [[ $revision != 1 ]]; then
  echo "make-source: revision $revision reuses the orig of $upstream-1: set SWAYDESK_ORIG (osc checkout)" >&2
  exit 1
else
  "$root/packaging/make-orig.sh" "$work" >/dev/null
fi

# O .dsc: a árvore do tarball com o debian/ do commit (o git archive, como o make-orig: nada da árvore de
# trabalho entra)
tar -xJf "$tarball" -C "$work"
git -C "$root" archive --format=tar HEAD debian | tar -x -C "$work/swaydesk-$upstream"
"$root/packaging/version.sh" changelog "$work/swaydesk-$upstream/debian/changelog"
(cd "$work" && dpkg-source --build "swaydesk-$upstream" >/dev/null)

git -C "$root" archive --format=tar HEAD packaging/arch | tar -x -C "$work"
python3 "$root/packaging/obs/pkgbuild.py" "$work/packaging/arch/PKGBUILD" "$arch" "$tarball" "$revision" > "$out/PKGBUILD"
cp "$work"/packaging/arch/*.install "$out/"
mv "$tarball" "$work"/swaydesk_"$deb".dsc "$work"/swaydesk_"$deb".debian.tar.xz "$out/"

git -C "$root" archive --format=tar HEAD packaging/obs/keyring | tar -x -C "$work"
(cd "$work/packaging/obs" && dpkg-source --build keyring >/dev/null && mv swaydesk-archive-keyring_* "$keyring/")

# O adw-gtk-theme: a versão do debian/changelog, o orig é o tarball do upstream (o mesmo sha256 sempre, então o
# push.sh vê o mesmo conteúdo e não reenvia). Sem curl na imagem de build: o urllib do python3.
git -C "$root" archive --format=tar HEAD packaging/obs/adw-gtk-theme | tar -x -C "$work"
adw_src=$work/packaging/obs/adw-gtk-theme
adw_version=$(dpkg-parsechangelog -l "$adw_src/debian/changelog" -S Version)
adw_upstream=${adw_version%-*}
adw_url=$(sed -n 's/^url=//p' "$adw_src/upstream") adw_sha=$(sed -n 's/^sha256=//p' "$adw_src/upstream")
adw_orig=$work/adw-gtk-theme_$adw_upstream.orig.tar.xz
python3 -I -c 'import shutil, sys, urllib.request
with urllib.request.urlopen(sys.argv[1], timeout=60) as r, open(sys.argv[2], "wb") as f: shutil.copyfileobj(r, f)' "$adw_url" "$adw_orig"
echo "$adw_sha  $adw_orig" | sha256sum --check --quiet - || { echo "make-source: $adw_url: wrong sha256" >&2; exit 1; }
mkdir "$work/adw-gtk-theme-$adw_upstream"
tar -xJf "$adw_orig" -C "$work/adw-gtk-theme-$adw_upstream"
cp -r "$adw_src/debian" "$work/adw-gtk-theme-$adw_upstream/"
(cd "$work" && dpkg-source --build "adw-gtk-theme-$adw_upstream" >/dev/null)
mv "$adw_orig" "$work"/adw-gtk-theme_"$adw_version".dsc "$work"/adw-gtk-theme_"$adw_version".debian.tar.xz "$adw/"
(cd "$base" && ls -- */)
