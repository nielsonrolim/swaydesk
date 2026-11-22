#!/usr/bin/env bash
# Envia ao OBS uma pasta do make-source.sh: cada subpasta (swaydesk, swaydesk-archive-keyring) vira o
# conteúdo do pacote de mesmo nome no projeto, num commit do osc. Os arquivos que saíram da pasta saem
# também do pacote. Um pacote cujo .dsc (a mesma versão) já está no OBS fica como está. O osc usa a conta configurada (osc whois); a senha fica no chaveiro.
# Até a 0.1.0, o projeto recebe snapshots do main (X.Y.Z~git…). Depois dela, só lançamentos: um snapshot
# não vai para um pacote que já tem um lançamento (versão sem ~git) no OBS, porque quem instalou a versão
# estável receberia o desenvolvimento no próximo upgrade.
# Não precisa do dpkg (roda num PC Arch): as versões são comparadas pelo debversion.py, e os fontes extraídos
# pelo tar, a partir da lista Files: do .dsc.
# uso: packaging/obs/push.sh SAÍDA [PACOTE...]   (padrão: todos os da pasta)
#      OBS_PROJECT=home:outro packaging/obs/push.sh ...   (padrão: home:nielsonrolim)
set -euo pipefail
base=$(cd "${1:?uso: push.sh SAÍDA [PACOTE...]}" && pwd)
shift
project=${OBS_PROJECT:-home:nielsonrolim}
if (($#)); then packages=("$@"); else mapfile -t packages < <(cd "$base" && ls -d -- */ | tr -d /); fi
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# O fonte de um .dsc numa pasta, como o dpkg-source -x: os tarballs da lista Files:, o upstream (ou o nativo)
# sem a pasta de cima e a debian/ do debian.tar no lugar da que houver. Uma assinatura (.asc) não entra. Só o
# que os nossos pacotes usam: um tarball de componente (orig-NOME) ou patches na debian/ são recusados, e
# também um nome com / ou caracteres de glob
extract_dsc() {
  local dsc=$1 dir=$2 from name names=()
  from=$(dirname "$dsc")
  mapfile -t names < <(sed -n '/^Files:/,/^[^ ]/s/^ [0-9a-f]\{32\} [0-9]* //p' "$dsc")
  ((${#names[@]})) || { echo "push: $dsc lists no files" >&2; return 1; }
  mkdir -p "$dir"
  # O upstream (ou o nativo) antes do debian.tar, seja qual for a ordem da lista
  for name in "${names[@]}"; do
    [[ $name =~ ^[A-Za-z0-9._+~-]+$ && -f $from/$name ]] || { echo "push: $dsc: bad or missing file $name" >&2; return 1; }
    case $name in
      *.asc | *.debian.tar.*) ;;
      *.orig-*.tar.*) echo "push: $dsc: component tarballs are not supported ($name)" >&2; return 1 ;;
      *.tar.*)
        # Uma pasta só no topo, como o dpkg-source exige: o --strip-components jogaria fora o que estivesse fora dela
        [[ $(tar -tf "$from/$name" | sed 's|^\./||; s|/.*||' | sort -u | wc -l) == 1 ]] ||
          { echo "push: $dsc: $name has no single top folder" >&2; return 1; }
        tar -xf "$from/$name" -C "$dir" --strip-components=1 ;;
      *) echo "push: $dsc: unsupported file $name" >&2; return 1 ;;
    esac
  done
  for name in "${names[@]}"; do
    case $name in *.debian.tar.*) rm -rf "$dir/debian" && tar -xf "$from/$name" -C "$dir" ;; esac
  done
  [[ ! -e $dir/debian/patches/series ]] || { echo "push: $dsc: patches are not supported" >&2; return 1; }
}

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
      extract_dsc "$PWD/$dsc" "$work/old-$pkg"
      extract_dsc "$src/$dsc" "$work/new-$pkg"
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
      # 0: menor; 1: não; outro código: uma versão inválida, que também para o envio
      rc=0; python3 -I "$here/debversion.py" "$new_version" lt "$old_version" || rc=$?
      if ((rc == 0)); then
        echo "push: $project/$pkg: $new_version is below $old_version, already on the OBS" >&2
        exit 1
      elif ((rc != 1)); then
        echo "push: $project/$pkg: could not compare $new_version with $old_version" >&2
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
