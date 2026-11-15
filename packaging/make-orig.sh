#!/usr/bin/env bash
# O tarball de origem para o Debian: o commit atual (git archive) com os crates do rust/ copiados para
# rust/vendor (cargo vendor) e o rust/.cargo/config.toml que os usa. O Launchpad compila sem rede, então
# tudo o que o cargo baixa precisa estar dentro. Gera ../swaydesk_<versão>.orig.tar.xz, ao lado do repositório.
# Usa o commit atual (HEAD), e não a árvore de trabalho: faça o commit antes.
# Só leva o que o build usa: rust/, packaging/, o LICENSE e os arquivos que o packaging/install-tree.sh lê
# (--sources). Os dotfiles, a documentação interna e os testes do repositório ficam de fora: o tarball é
# público no OBS.
# uso: packaging/make-orig.sh [PASTA_DE_SAÍDA]
set -euo pipefail
root=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
out=${1:-$(dirname "$root")}
version=$("$root/packaging/version.sh")
name=swaydesk-$version
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# O commit inteiro numa pasta, e dela só a lista (o --sources do próprio commit). O debian/ vai no
# .debian.tar, fora do tarball de origem
git -C "$root" archive --format=tar --prefix=full/ HEAD | tar -x -C "$work"
{ printf '%s\n' rust packaging LICENSE; "$work/full/packaging/install-tree.sh" --sources; } > "$work/list"
mkdir "$work/$name"
tar -C "$work/full" -c --verbatim-files-from -T "$work/list" | tar -x -C "$work/$name"
rm -rf "${work:?}/full"
(cd "$work/$name/rust" && mkdir -p .cargo && cargo vendor --locked --versioned-dirs vendor > .cargo/config.toml 2>/dev/null)
# Sem o --quiet: ele também cala a config que o cargo vendor escreve na saída, e o config.toml sairia vazio
grep -q "vendored-sources" "$work/$name/rust/.cargo/config.toml" || { echo "make-orig: cargo vendor wrote no config" >&2; exit 1; }
tar -C "$work" -cJf "$out/swaydesk_$version.orig.tar.xz" "$name"
echo "$out/swaydesk_$version.orig.tar.xz"
