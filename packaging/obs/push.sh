#!/usr/bin/env bash
# Envia ao OBS uma pasta do make-source.sh: cada subpasta (swaydesk, swaydesk-archive-keyring) vira o
# conteúdo do pacote de mesmo nome no projeto, num commit do osc. Os arquivos que saíram da pasta saem
# também do pacote. Um pacote cujo .dsc (a mesma versão) já está no OBS fica como está. O osc usa a conta configurada (osc whois); a senha fica no chaveiro.
# Até a 0.1.0, o projeto recebe snapshots do main (X.Y.Z~git…). Depois dela, só lançamentos: um snapshot
# não vai para um pacote que já tem um lançamento (versão sem ~git) no OBS, porque quem instalou a versão
# estável receberia o desenvolvimento no próximo upgrade.
# uso: packaging/obs/push.sh SAÍDA [PACOTE...]   (padrão: todos os da pasta)
#      OBS_PROJECT=home:outro packaging/obs/push.sh ...   (padrão: home:nielsonrolim)
set -euo pipefail
base=$(cd "${1:?uso: push.sh SAÍDA [PACOTE...]}" && pwd)
shift
project=${OBS_PROJECT:-home:nielsonrolim}
if (($#)); then packages=("$@"); else mapfile -t packages < <(cd "$base" && ls -d -- */ | tr -d /); fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

for pkg in "${packages[@]}"; do
  src=$base/$pkg
  [[ -d $src ]] || { echo "push: no folder $src" >&2; exit 1; }
  dsc=$(cd "$src" && ls -- *.dsc)
  (
    cd "$work"
    osc checkout --output-dir "$pkg" "$project" "$pkg" >/dev/null
    cd "$pkg"
    # A mesma versão já está lá: não reenvia. O tarball gerado de novo muda (as datas dos arquivos), e o OBS
    # publicaria outro .deb com a mesma versão. Uma mudança no pacote pede uma versão nova no changelog
    # Os bytes do tarball sempre mudam, então compara o conteúdo extraído: diferente na mesma versão é um erro
    if [[ -e $dsc ]]; then
      dpkg-source --no-check -x "$dsc" "$work/old-$pkg" >/dev/null
      dpkg-source --no-check -x "$src/$dsc" "$work/new-$pkg" >/dev/null
      if ! diff -r "$work/old-$pkg" "$work/new-$pkg" >/dev/null; then
        echo "push: $project/$pkg: same version ($dsc), different content: bump its debian/changelog" >&2
        exit 1
      fi
      echo "push: $project/$pkg: $dsc is already there, with the same content: skipped"
      exit 0
    fi
    # Uma versão menor que a do OBS não chegaria a quem já tem a de lá (o apt e o pacman não fazem downgrade)
    for old in *.dsc; do
      [[ -e $old ]] || continue
      old_version=$(sed -n 's/^Version: //p' "$old") new_version=$(sed -n 's/^Version: //p' "$src/$dsc")
      if [[ $pkg == swaydesk && $new_version == *~git* && $old_version != *~git* ]]; then
        echo "push: $project/$pkg: $old_version is a release; after it, only releases (tags) go to the OBS" >&2
        exit 1
      fi
      if dpkg --compare-versions "$new_version" lt "$old_version"; then
        echo "push: $project/$pkg: $new_version is below $old_version, already on the OBS" >&2
        exit 1
      fi
    done
    # Só os arquivos do pacote: o .osc é do osc, e os que começam com _ (um _service, se um dia houver)
    # não vêm do make-source.sh
    find . -maxdepth 1 -type f ! -name '_*' -delete
    cp -- "$src"/* .
    osc addremove >/dev/null
    if [[ -z $(osc status) ]]; then
      echo "push: $project/$pkg: nothing changed"
    else
      osc status
      osc commit --message "${dsc%.dsc}"
      if [[ $pkg == swaydesk-archive-keyring ]]; then
        echo "push: the keyring .deb gets a new sha256 when the OBS builds it: update keyring-published.sha256," >&2
        echo "push: the public READMEs and the site, then run packaging/obs/check-keyring.sh" >&2
      fi
    fi
  )
done
