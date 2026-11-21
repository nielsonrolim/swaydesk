#!/usr/bin/env bash
# Anexa as ISOs de um lançamento ao Release vX.Y.Z do espelho público (github.com/nielsonrolim/swaydesk), com um
# SHA256SUMS (só SHA-256, sem assinatura GPG) e uma seção "ISOs" no fim do texto do Release. O SHA256SUMS no
# mesmo Release protege contra um download corrompido ou cortado, não contra quem controla o Release: não é
# uma assinatura (decisão do usuário: sem GPG).
#
# Cada ISO entra com o nome swaydesk-X.Y.Z-<distro>.iso (debian13-amd64, debiantesting-amd64, ubuntu26.04-amd64,
# arch-x86_64). O
# GitHub não aceita um arquivo de Release com mais de 2 GiB: uma ISO acima disso NÃO é publicada (nem aqui nem
# na página de download do site; decisão do usuário: nada de dividir, enxugar ou outro servidor). O script
# avisa qual ficou de fora e com que tamanho, e segue com as outras.
#
# Depois do envio, baixa do Release o que acabou de subir e confere com o SHA256SUMS: o que o usuário baixa é
# o que foi conferido aqui. Um Release que já tem um desses arquivos não é sobrescrito (nada de --clobber).
#
# uso: packaging/release-isos.sh [--dry-run] vX.Y.Z PASTA debian13=ISO [debiantesting=ISO] ubuntu2604=ISO arch=ISO
#      packaging/release-isos.sh [--dry-run] --add vX.Y.Z PASTA arch=ISO
#      PASTA recebe os nomes finais (links físicos ou cópias), o SHA256SUMS e a conferência do download: fica
#      fora do /tmp, porque são gigabytes. Com --dry-run, monta e confere a PASTA e mostra o que enviaria.
#
# Com --add, uma ISO que ficou pronta depois entra num Release que já tem as outras: o Release tem de ter o
# SHA256SUMS e a seção "ISOs", e não a ISO nova. As linhas do SHA256SUMS de lá ficam byte a byte, e a nova vai
# no fim. O SHA256SUMS novo sobe como SHA256SUMS.new e troca de lugar com o antigo por dois renomes da API
# (o antigo vira SHA256SUMS.old, o novo vira SHA256SUMS, e o .old sai): sem o --clobber, que apagaria o
# arquivo antes de subir o outro. Entre os dois renomes (uma fração de segundo), um download do SHA256SUMS dá
# 404; o GitHub não troca dois arquivos de uma vez. Antes da troca, baixa e confere a ISO nova; depois dela,
# baixa TODAS as ISOs do SHA256SUMS e confere.
# GH troca o comando gh (testes); ISO_REPO troca o espelho; ISO_LIMIT troca o limite em bytes (testes).
set -euo pipefail
gh=${GH:-gh}
repo=${ISO_REPO:-nielsonrolim/swaydesk}
limit=${ISO_LIMIT:-2147483648}  # 2 GiB: o GitHub só aceita um arquivo de Release abaixo disso

dry=0 add=0
while [[ ${1:-} == --* ]]; do
  case $1 in
    --dry-run) dry=1 ;;
    --add) add=1 ;;
    *) echo "release-isos: unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done
usage() {
  echo "uso: packaging/release-isos.sh [--dry-run] vX.Y.Z PASTA debian13=ISO [debiantesting=ISO] ubuntu2604=ISO arch=ISO" >&2
  echo "     packaging/release-isos.sh [--dry-run] --add vX.Y.Z PASTA DISTRO=ISO" >&2
  exit 2
}
(($# >= 3)) || usage
tag=$1 out=$2
shift 2
[[ $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]] || { echo "release-isos: the tag must be vX.Y.Z, not $tag" >&2; exit 2; }
version=${BASH_REMATCH[1]}

declare -A suffix=([debian13]=debian13-amd64 [debiantesting]=debiantesting-amd64 [ubuntu2604]=ubuntu26.04-amd64
                   [arch]=arch-x86_64)
declare -A label=([debian13]="Debian 13" [debiantesting]="Debian Testing" [ubuntu2604]="Ubuntu 26.04"
                  [arch]="Arch Linux")
order=(debian13 debiantesting ubuntu2604 arch)
declare -A source=()
for arg in "$@"; do
  key=${arg%%=*} path=${arg#*=}
  [[ $arg == *=* && -n ${suffix[$key]+x} ]] || { echo "release-isos: unknown argument: $arg" >&2; usage; }
  [[ -z ${source[$key]+x} ]] || { echo "release-isos: $key given twice" >&2; exit 2; }
  [[ -f $path ]] || { echo "release-isos: $key: no file $path" >&2; exit 1; }
  source[$key]=$path
done

# O Release tem de existir (o mirror.sh --tag e o gh release create vêm antes) e não pode já ter as ISOs
assets=$("$gh" release view "$tag" -R "$repo" --json assets -q '.assets[].name')
mkdir -p "$out"
[[ -z $(ls -A "$out") ]] || { echo "release-isos: $out is not empty" >&2; exit 1; }

if ((add)); then
  # Só as ISOs dadas não podem estar lá; o SHA256SUMS tem de estar, e nenhuma sobra de um --add que parou
  for key in "${!source[@]}"; do
    name=swaydesk-$version-${suffix[$key]}.iso
    if grep -qxF "$name" <<<"$assets"; then
      echo "release-isos: $tag already has $name; nothing sent" >&2; exit 1
    fi
  done
  grep -qxF SHA256SUMS <<<"$assets" || { echo "release-isos: --add: $tag has no SHA256SUMS; run without --add" >&2; exit 1; }
  for name in SHA256SUMS.new SHA256SUMS.old; do
    if grep -qxF "$name" <<<"$assets"; then
      echo "release-isos: $tag has a $name left over from a stopped --add; remove it first" >&2; exit 1
    fi
  done
else
  for name in "${suffix[@]}"; do
    name=swaydesk-$version-$name.iso
    if grep -qxF "$name" <<<"$assets"; then
      echo "release-isos: $tag already has $name; nothing sent" >&2; exit 1
    fi
  done
  if grep -qxF SHA256SUMS <<<"$assets"; then
    echo "release-isos: $tag already has SHA256SUMS; nothing sent" >&2; exit 1
  fi
fi

# --add: o SHA256SUMS de lá + as linhas novas, a linha nova na seção "ISOs", o envio, a troca e a conferência
add_mode() {
  mkdir "$out/current"
  "$gh" release download "$tag" -R "$repo" -D "$out/current" -p SHA256SUMS
  local old=$out/current/SHA256SUMS line name
  [[ -s $old && -z $(tail -c 1 "$old") ]] \
    || { echo "release-isos: --add: $tag's SHA256SUMS is empty or doesn't end in a newline" >&2; exit 1; }
  while IFS= read -r line; do
    [[ $line =~ ^[0-9a-f]{64}\ \ (swaydesk-$version-[a-z0-9._-]+\.iso)$ ]] \
      || { echo "release-isos: --add: unexpected line in $tag's SHA256SUMS: $line" >&2; exit 1; }
    name=${BASH_REMATCH[1]}
    for f in "${files[@]}"; do
      [[ $f != "$name" ]] || { echo "release-isos: --add: $tag's SHA256SUMS already lists $name" >&2; exit 1; }
    done
  done < "$old"
  { cat "$old"; (cd "$out" && sha256sum -- "${files[@]}"); } > "$out/SHA256SUMS.new"
  (cd "$out" && sha256sum -c --quiet --ignore-missing SHA256SUMS.new)
  cmp -s -n "$(stat -c %s "$old")" "$old" "$out/SHA256SUMS.new" \
    || { echo "release-isos: --add: the old lines changed" >&2; exit 1; }
  echo "== SHA256SUMS (new)"; cat "$out/SHA256SUMS.new"

  # A seção "ISOs" do texto ganha as linhas novas na ordem das distros: cada uma antes da primeira linha de ISO
  # que vem depois dela em order (a de Debian Testing antes da do Ubuntu), ou depois da última linha de ISO
  local body
  body=$("$gh" release view "$tag" -R "$repo" --json body -q .body)
  grep -qxF '### ISOs' <<<"$body" || { echo "release-isos: --add: $tag's text has no ISOs section" >&2; exit 1; }
  local entries=() ranks=() key i
  for i in "${!order[@]}"; do
    key=${order[$i]} name=swaydesk-$version-${suffix[$key]}.iso
    ranks+=("$i ${name}")
    for f in "${files[@]}"; do if [[ $f == "$name" ]]; then entries+=("$i"$'\t'"- \`$name\`: ${label[$key]}"); fi; done
  done
  printf '%s\n' "$body" | awk -v new="$(printf '%s\n' "${entries[@]}")" -v ranks="$(printf '%s\n' "${ranks[@]}")" \
      -v pat="^- \`swaydesk-$version-" '
    BEGIN { n = split(new, e, "\n"); for (j = 1; j <= n; j++) { r[j] = substr(e[j], 1, index(e[j], "\t") - 1) + 0
                                                                  e[j] = substr(e[j], index(e[j], "\t") + 1) }
            m = split(ranks, k, "\n"); for (j = 1; j <= m; j++) { split(k[j], kv, " "); rank["- `" kv[2] "`"] = kv[1] + 0 } }
    { lines[NR] = $0 } $0 ~ pat { last = NR }
    END { if (!last) exit 1
          for (i = 1; i <= NR; i++) {
            if (lines[i] ~ pat) {
              mine = lines[i]; sub(/: .*/, "", mine)
              if (mine in rank) for (j = 1; j <= n; j++) if (!done[j] && r[j] < rank[mine]) { print e[j]; done[j] = 1 }
            }
            print lines[i]
            if (i == last) for (j = 1; j <= n; j++) if (!done[j]) { print e[j]; done[j] = 1 }
          } }' > "$out/release-body.md" \
    || { echo "release-isos: --add: no ISO line in $tag's ISOs section" >&2; exit 1; }
  echo "== Release text (new)"; cat "$out/release-body.md"

  if ((dry)); then
    echo "release-isos: dry run; would upload to $repo $tag: ${files[*]}, and swap in the new SHA256SUMS"
    return 0
  fi

  add_recovery() {
    echo "release-isos: FAILED during --add. Remove what this run added and run again with an empty folder:" >&2
    for name in "${files[@]}" SHA256SUMS.new; do echo "  $gh release delete-asset $tag $name -R $repo -y" >&2; done
  }
  trap add_recovery ERR
  (cd "$out" && "$gh" release upload "$tag" -R "$repo" -- "${files[@]}" SHA256SUMS.new)
  # Antes da troca, o que subiu agora confere com o SHA256SUMS.new: uma ISO estragada não chega a ser listada
  mkdir "$out/uploaded"
  local patterns=(-p SHA256SUMS.new)
  for name in "${files[@]}"; do patterns+=(-p "$name"); done
  "$gh" release download "$tag" -R "$repo" -D "$out/uploaded" "${patterns[@]}"
  cmp -s "$out/SHA256SUMS.new" "$out/uploaded/SHA256SUMS.new"
  (cd "$out/uploaded" && sha256sum -c --ignore-missing SHA256SUMS.new)

  # A troca: dois renomes pela API. Se o segundo falhar, o antigo volta ao nome
  local ids old_id new_id
  ids=$("$gh" api "repos/$repo/releases/tags/$tag" --jq '.assets[] | "\(.id) \(.name)"')
  old_id=$(awk '$2 == "SHA256SUMS" { print $1 }' <<<"$ids")
  new_id=$(awk '$2 == "SHA256SUMS.new" { print $1 }' <<<"$ids")
  [[ -n $old_id && -n $new_id ]] || { echo "release-isos: --add: can't find the SHA256SUMS assets" >&2; add_recovery; exit 1; }
  trap - ERR
  # Devolve o nome ao antigo, se ele estiver como SHA256SUMS.old (um erro pode vir depois de o GitHub ter
  # aplicado o renome, num timeout): o Release nunca fica sem SHA256SUMS sem que o script diga
  name_of() {  # o nome atual do asset $1, ou nada
    "$gh" api "repos/$repo/releases/tags/$tag" --jq '.assets[] | "\(.id) \(.name)"' 2>/dev/null \
      | awk -v id="$1" '$1 == id { print $2 }' || true
  }
  put_back() {
    local now
    now=$(name_of "$old_id")
    [[ $now == SHA256SUMS ]] && return 0
    if [[ $now == SHA256SUMS.old ]] \
       && "$gh" api -X PATCH "repos/$repo/releases/assets/$old_id" -f name=SHA256SUMS >/dev/null; then
      echo "release-isos: the old SHA256SUMS has its name back" >&2; return 0
    fi
    echo "release-isos: PUT IT BACK BY HAND: $gh api -X PATCH repos/$repo/releases/assets/$old_id -f name=SHA256SUMS" >&2
  }
  if ! "$gh" api -X PATCH "repos/$repo/releases/assets/$old_id" -f name=SHA256SUMS.old >/dev/null; then
    echo "release-isos: --add: renaming the old SHA256SUMS failed" >&2; put_back; add_recovery; exit 1
  fi
  if ! "$gh" api -X PATCH "repos/$repo/releases/assets/$new_id" -f name=SHA256SUMS >/dev/null; then
    # Um erro com o renome aplicado (um timeout) é uma troca que deu certo: o novo já foi conferido, segue
    if [[ $(name_of "$new_id") == SHA256SUMS ]]; then
      echo "release-isos: --add: the rename of SHA256SUMS.new reported an error but was applied; going on" >&2
    else
      echo "release-isos: --add: renaming SHA256SUMS.new failed; putting the old one back" >&2
      put_back; add_recovery; exit 1
    fi
  fi

  # O novo está no lugar: daqui em diante não se apaga nada que esteja publicado
  add_check_failed() {
    echo "release-isos: the new SHA256SUMS is in place, but a later step failed. Check by hand:" >&2
    echo "  $gh release delete-asset $tag SHA256SUMS.old -R $repo -y   (if it is still there)" >&2
    echo "  $gh release download $tag -R $repo -D DIR; (cd DIR && sha256sum -c SHA256SUMS)" >&2
    echo "  $gh release edit $tag -R $repo --notes-file $out/release-body.md   (the text)" >&2
  }
  trap add_check_failed ERR
  "$gh" release delete-asset "$tag" SHA256SUMS.old -R "$repo" -y
  mkdir "$out/download"
  patterns=(-p SHA256SUMS)
  while IFS= read -r line; do patterns+=(-p "${line#*  }"); done < "$out/SHA256SUMS.new"
  "$gh" release download "$tag" -R "$repo" -D "$out/download" "${patterns[@]}"
  if ! cmp -s "$out/SHA256SUMS.new" "$out/download/SHA256SUMS"; then
    echo "release-isos: the downloaded SHA256SUMS differs" >&2; add_check_failed; exit 1
  fi
  (cd "$out/download" && sha256sum -c SHA256SUMS)
  "$gh" release edit "$tag" -R "$repo" --notes-file "$out/release-body.md" >/dev/null
  trap - ERR
  echo "release-isos: $tag now has ${files[*]}; SHA256SUMS lists all its ISOs and they all check"
  for item in "${left[@]}"; do echo "release-isos: LEFT OUT: $item" >&2; done
}

files=() left=()
for key in "${order[@]}"; do
  [[ -n ${source[$key]+x} ]] || continue
  name=swaydesk-$version-${suffix[$key]}.iso
  size=$(stat -c %s "${source[$key]}")
  if ((size >= limit)); then
    left+=("$name ($size bytes, not under the $limit-byte limit)")
    continue
  fi
  ln "${source[$key]}" "$out/$name" 2>/dev/null || cp --reflink=auto -- "${source[$key]}" "$out/$name"
  files+=("$name")
done
for item in "${left[@]}"; do echo "release-isos: LEFT OUT: $item" >&2; done
((${#files[@]})) || { echo "release-isos: no ISO under the limit; nothing sent" >&2; exit 1; }

if ((add)); then
  add_mode
  exit 0
fi

(cd "$out" && sha256sum -- "${files[@]}" > SHA256SUMS && sha256sum -c --quiet SHA256SUMS)
echo "== SHA256SUMS"; cat "$out/SHA256SUMS"

# A seção do texto do Release: só as ISOs publicadas
section=$'\n### ISOs\n\nLive images that also install Swaydesk:\n\n'
for key in "${order[@]}"; do
  name=swaydesk-$version-${suffix[$key]}.iso
  for f in "${files[@]}"; do if [[ $f == "$name" ]]; then section+="- \`$name\`: ${label[$key]}"$'\n'; fi; done
done
section+=$'\nCheck a download before writing it to a USB stick (in the folder with the ISO and `SHA256SUMS`). It catches a corrupted or incomplete download; it is not a signature.\n\n```bash\nsha256sum -c --ignore-missing SHA256SUMS\n```\n'
printf '%s' "$section" > "$out/release-section.md"
echo "== Release section"; cat "$out/release-section.md"

if ((dry)); then
  echo "release-isos: dry run; would upload to $repo $tag: ${files[*]} SHA256SUMS"
  exit 0
fi

# Do envio em diante, uma falha pode deixar arquivos públicos sem a seção no texto: mostra como tirá-los
recovery() {
  echo "release-isos: FAILED after the upload started. The Release may have some of these files; remove them" >&2
  echo "release-isos: and run again with an empty folder:" >&2
  for name in "${files[@]}" SHA256SUMS; do echo "  $gh release delete-asset $tag $name -R $repo -y" >&2; done
}
trap recovery ERR
(cd "$out" && "$gh" release upload "$tag" -R "$repo" -- "${files[@]}" SHA256SUMS)

# O que o GitHub serve agora é o que foi conferido
mkdir "$out/download"
patterns=()
for name in "${files[@]}" SHA256SUMS; do patterns+=(-p "$name"); done
"$gh" release download "$tag" -R "$repo" -D "$out/download" "${patterns[@]}"
if ! cmp -s "$out/SHA256SUMS" "$out/download/SHA256SUMS"; then
  echo "release-isos: the downloaded SHA256SUMS differs" >&2; recovery; exit 1
fi
(cd "$out/download" && sha256sum -c SHA256SUMS)

# Daqui em diante, os arquivos estão publicados e conferidos: uma falha é só do texto, e não se apaga nada
text_failed() {
  echo "release-isos: the files are published and verified; only the Release text failed. Don't delete them." >&2
  echo "release-isos: append $out/release-section.md to the Release text, e.g.:" >&2
  echo "  { $gh release view $tag -R $repo --json body -q .body; cat $out/release-section.md; } > $out/release-body.md" >&2
  echo "  $gh release edit $tag -R $repo --notes-file $out/release-body.md" >&2
}
trap text_failed ERR
body=$("$gh" release view "$tag" -R "$repo" --json body -q .body)
if grep -qxF '### ISOs' <<<"$body"; then
  echo "release-isos: the Release text already has an ISOs section; left as it is" >&2
else
  printf '%s\n%s' "$body" "$section" > "$out/release-body.md"
  "$gh" release edit "$tag" -R "$repo" --notes-file "$out/release-body.md" >/dev/null
fi
trap - ERR
echo "release-isos: $tag has ${files[*]} and SHA256SUMS"
for item in "${left[@]}"; do echo "release-isos: LEFT OUT: $item" >&2; done
