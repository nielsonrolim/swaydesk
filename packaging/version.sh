#!/usr/bin/env bash
# A versão dos pacotes, tirada do commit atual (HEAD). A workspace.version do rust/Cargo.toml é a próxima
# versão a lançar, e o lançamento é a tag v<versão>:
#   - na tag: a própria versão (0.1.0; o .deb sai 0.1.0-1);
#   - fora dela, um snapshot abaixo do lançamento, que cresce com a data do commit (a do committer, em UTC,
#     AAAAMMDDhhmmss):
#       Debian  0.1.0~git<data>.<sha7>   (o ~ fica antes do 0.1.0 no dpkg)
#       Arch    0.1.0git<data>.g<sha7>   (no vercmp do pacman, letras coladas no número ficam antes dele, como
#                                        num 1.0rc; com um ponto, 0.1.0.git, ficariam depois)
#     A data, e não o número de commits: o squash do GitHub junta os commits de uma branch num só, e o main
#     ficaria com menos commits que a branch que já tinha ido ao OBS. O committer de um merge é a hora do merge
#     (o autor pode ser a do primeiro commit da branch), sempre depois dos commits da branch. Os snapshots
#     antigos, 0.1.0~git<N> com N de três dígitos, ficam abaixo de qualquer data.
#   Depois de lançar, o Cargo sobe para a próxima versão no commit logo após a tag: um snapshot da 0.1.0
#   depois da v0.1.0 ficaria abaixo dela, e o apt e o pacman não o instalariam. O script recusa esse caso.
#   Uma revisão só do empacotamento é a tag v<versão>-N, num commit de uma branch de manutenção a partir da
#   tag do lançamento (release/X.Y): o .deb sai <versão>-N e o pacote do Arch, pkgver <versão> e pkgrel N.
#   Os nomes de arquivo mudam, o que importa no OBS: um CDN que guardou o -1 não serve o -2 velho.
# uso: packaging/version.sh [upstream|deb|arch|revision]   (padrão: upstream, a versão do Debian sem a
#                                                  revisão; revision: o N do .deb e o pkgrel do Arch)
#      packaging/version.sh changelog ARQUIVO      põe a entrada do snapshot no topo de um debian/changelog
#                                                  (uma cópia: o do repositório fica com a próxima versão)
set -euo pipefail
root=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
release=$(awk -F'"' '/^\[workspace.package\]/ { found = 1 } found && /^version/ { print $2; exit }' "$root/rust/Cargo.toml")
[[ $release =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version: unexpected Cargo version: $release" >&2; exit 1; }
revision=1

# A tag do lançamento no HEAD: v<versão> (a revisão 1) ou v<versão>-N (a revisão N, N >= 2)
tag=$(git -C "$root" tag --points-at HEAD | grep -xE "v${release//./\\.}(-[0-9]+)?" | sort -V | tail -n1 || true)
if [[ -n $tag ]]; then
  upstream=$release arch=$release
  if [[ $tag == *-* ]]; then
    revision=${tag##*-}
    [[ $revision =~ ^[1-9][0-9]*$ && $revision -ge 2 ]] || { echo "version: unexpected revision tag: $tag" >&2; exit 1; }
  fi
elif git -C "$root" tag --merged HEAD | grep -qx "v$release"; then
  echo "version: HEAD is past the tag v$release, but rust/Cargo.toml is still $release: bump it first" >&2
  exit 1
else
  stamp=$(TZ=UTC git -C "$root" log -1 --format=%cd --date=format-local:%Y%m%d%H%M%S HEAD)
  sha=$(git -C "$root" rev-parse --short=7 HEAD)
  upstream="$release~git$stamp.$sha" arch="${release}git$stamp.g$sha"
fi

case ${1:-upstream} in
  upstream) echo "$upstream" ;;
  deb) echo "$upstream-$revision" ;;
  arch) echo "$arch" ;;
  revision) echo "$revision" ;;
  changelog)
    file=${2:?uso: version.sh changelog ARQUIVO}
    top=$(head -n1 "$file")
    [[ $top == "swaydesk ($release-"* ]] || { echo "version: $file does not start at $release: $top" >&2; exit 1; }
    if [[ $upstream == "$release" ]]; then  # o lançamento usa a entrada que está no arquivo, já fechada
      [[ $top != *UNRELEASED* ]] || { echo "version: the v$release tag, but $file is still UNRELEASED" >&2; exit 1; }
      [[ $top == "swaydesk ($release-$revision) "* ]] || {
        echo "version: the tag is $release-$revision, but $file starts at: $top" >&2; exit 1; }
      exit 0
    fi
    # O mantenedor da entrada de cima (o e-mail público do changelog) e a data do commit: a mesma fonte
    # gera sempre o mesmo changelog
    maintainer=$(grep -m1 '^ -- ' "$file" | sed -E 's/^ -- (.*>)  .*/\1/')
    date=$(git -C "$root" log -1 --format=%cD HEAD)
    full=$(git -C "$root" rev-parse HEAD)
    tmp=$(mktemp)
    printf 'swaydesk (%s-%s) unstable; urgency=medium\n\n  * Snapshot of commit %s.\n\n -- %s  %s\n\n' \
      "$upstream" "$revision" "$full" "$maintainer" "$date" > "$tmp"
    cat "$file" >> "$tmp"
    mv "$tmp" "$file"
    ;;
  *) echo "uso: version.sh [upstream|deb|arch|revision] | changelog ARQUIVO" >&2; exit 2 ;;
esac
