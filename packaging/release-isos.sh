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
#      packaging/release-isos.sh [--dry-run] --replace vX.Y.Z PASTA arch=ISO [ubuntu2604=ISO ...]
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
# Com --replace, uma ISO já publicada é trocada por outra (uma correção depois do lançamento): a ISO tem de
# estar no Release e no SHA256SUMS. Cada nova sobe como NOME.new e é baixada e conferida antes de qualquer
# troca; o SHA256SUMS novo muda só as linhas das ISOs trocadas, no lugar (as outras ficam byte a byte). Logo
# antes das trocas, o SHA256SUMS do Release é baixado de novo: se mudou desde o começo (outra troca no meio), para
# sem trocar nada. As trocas são renomes pela API (NOME -> NOME.old, NOME.new -> NOME; o SHA256SUMS por último);
# entre a troca de uma ISO e a do SHA256SUMS (segundos), a ISO nova não confere com o SHA256SUMS antigo. Depois,
# os .old saem e TODAS as ISOs do SHA256SUMS são baixadas e conferidas. O texto do Release não muda (mesmos nomes).
#
# Uma execução de cada vez nesta máquina: fora do --dry-run, o script segura um flock (ISO_LOCK).
# GH troca o comando gh (testes); ISO_REPO troca o espelho; ISO_LIMIT troca o limite em bytes (testes);
# ISO_LOCK troca o arquivo do flock (testes).
set -euo pipefail
gh=${GH:-gh}
repo=${ISO_REPO:-nielsonrolim/swaydesk}
limit=${ISO_LIMIT:-2147483648}  # 2 GiB: o GitHub só aceita um arquivo de Release abaixo disso

dry=0 add=0 replace=0
while [[ ${1:-} == --* ]]; do
  case $1 in
    --dry-run) dry=1 ;;
    --add) add=1 ;;
    --replace) replace=1 ;;
    *) echo "release-isos: unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done
usage() {
  echo "uso: packaging/release-isos.sh [--dry-run] vX.Y.Z PASTA debian13=ISO [debiantesting=ISO] ubuntu2604=ISO arch=ISO" >&2
  echo "     packaging/release-isos.sh [--dry-run] --add vX.Y.Z PASTA DISTRO=ISO" >&2
  echo "     packaging/release-isos.sh [--dry-run] --replace vX.Y.Z PASTA DISTRO=ISO [DISTRO=ISO ...]" >&2
  exit 2
}
((add + replace <= 1)) || { echo "release-isos: --add and --replace don't go together" >&2; exit 2; }
(($# >= 3)) || usage
tag=$1 out=$2
shift 2
[[ $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+(\.[0-9]+)?)$ ]] || { echo "release-isos: the tag must be vX.Y.Z or vX.Y.Z.W, not $tag" >&2; exit 2; }
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

# Uma execução de cada vez (o --dry-run não muda nada e não precisa): duas trocas ao mesmo tempo no mesmo
# SHA256SUMS fariam uma perder as linhas da outra
if ((!dry)); then
  lock=${ISO_LOCK:-${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/swaydesk-release.lock}
  exec 9>>"$lock"
  flock -n 9 || { echo "release-isos: another release-isos run holds $lock; nothing sent" >&2; exit 1; }
fi

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
elif ((replace)); then
  # As ISOs dadas têm de estar lá (e o SHA256SUMS), sem sobras de uma troca que parou
  grep -qxF SHA256SUMS <<<"$assets" || { echo "release-isos: --replace: $tag has no SHA256SUMS" >&2; exit 1; }
  for key in "${!source[@]}"; do
    name=swaydesk-$version-${suffix[$key]}.iso
    grep -qxF "$name" <<<"$assets" \
      || { echo "release-isos: --replace: $tag has no $name to replace; nothing sent" >&2; exit 1; }
    for leftover in "$name.new" "$name.old"; do
      if grep -qxF "$leftover" <<<"$assets"; then
        echo "release-isos: $tag has a $leftover left over from a stopped --replace; remove it first" >&2; exit 1
      fi
    done
  done
  for name in SHA256SUMS.new SHA256SUMS.old; do
    if grep -qxF "$name" <<<"$assets"; then
      echo "release-isos: $tag has a $name left over from a stopped run; remove it first" >&2; exit 1
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

# --replace: o SHA256SUMS de lá com as linhas trocadas no lugar, o envio como .new, a conferência, a checagem de
# que ninguém mexeu no meio, as trocas e a conferência final
replace_mode() {
  mkdir "$out/current"
  "$gh" release download "$tag" -R "$repo" -D "$out/current" -p SHA256SUMS
  local old=$out/current/SHA256SUMS line name f listed=()
  [[ -s $old && -z $(tail -c 1 "$old") ]] \
    || { echo "release-isos: --replace: $tag's SHA256SUMS is empty or doesn't end in a newline" >&2; exit 1; }
  while IFS= read -r line; do
    [[ $line =~ ^[0-9a-f]{64}\ \ (swaydesk-$version-[a-z0-9._-]+\.iso)$ ]] \
      || { echo "release-isos: --replace: unexpected line in $tag's SHA256SUMS: $line" >&2; exit 1; }
    listed+=("${BASH_REMATCH[1]}")
  done < "$old"
  for f in "${files[@]}"; do
    printf '%s\n' "${listed[@]}" | grep -qxF "$f" \
      || { echo "release-isos: --replace: $tag's SHA256SUMS doesn't list $f; nothing sent" >&2; exit 1; }
  done
  # Cada linha de uma ISO trocada ganha o sha novo, no mesmo lugar; as outras passam como estão
  local -A fresh=()
  for f in "${files[@]}"; do fresh[$f]=$(cd "$out" && sha256sum -- "$f" | cut -d' ' -f1); done
  while IFS= read -r line; do
    name=${line#*  }
    if [[ -n ${fresh[$name]+x} ]]; then printf '%s  %s\n' "${fresh[$name]}" "$name"; else printf '%s\n' "$line"; fi
  done < "$old" > "$out/SHA256SUMS.new"
  (cd "$out" && sha256sum -c --quiet --ignore-missing SHA256SUMS.new)
  # As outras linhas, byte a byte, e o mesmo número de linhas
  others() {  # as linhas de $1 que não são de uma ISO trocada
    awk -v skip="${files[*]}" 'BEGIN { split(skip, s, " "); for (i in s) drop[s[i]] = 1 }
      { n = $0; sub(/^[0-9a-f]+  /, "", n) } !(n in drop)' "$1"
  }
  cmp -s <(others "$old") <(others "$out/SHA256SUMS.new") \
    || { echo "release-isos: --replace: the other lines changed" >&2; exit 1; }
  [[ $(wc -l < "$old") == $(wc -l < "$out/SHA256SUMS.new") ]] \
    || { echo "release-isos: --replace: the number of lines changed" >&2; exit 1; }
  echo "== SHA256SUMS (new)"; cat "$out/SHA256SUMS.new"

  if ((dry)); then
    echo "release-isos: dry run; would replace in $repo $tag: ${files[*]}, and swap in the new SHA256SUMS"
    return 0
  fi

  # O envio: cada ISO como NOME.new (links na pasta up/, porque o gh dá ao asset o nome do arquivo)
  mkdir "$out/up"
  local uploads=()
  for f in "${files[@]}"; do ln "$out/$f" "$out/up/$f.new"; uploads+=("$f.new"); done
  ln "$out/SHA256SUMS.new" "$out/up/SHA256SUMS.new"
  uploads+=(SHA256SUMS.new)
  replace_recovery() {
    echo "release-isos: FAILED during --replace, before any swap: the published files are untouched. Remove what" >&2
    echo "release-isos: this run uploaded and run again with an empty folder:" >&2
    for name in "${uploads[@]}"; do echo "  $gh release delete-asset $tag $name -R $repo -y" >&2; done
  }
  trap replace_recovery ERR
  (cd "$out/up" && "$gh" release upload "$tag" -R "$repo" -- "${uploads[@]}")
  # O que subiu é o que está aqui, byte a byte: uma ISO estragada não chega a trocar de lugar
  mkdir "$out/uploaded"
  local patterns=()
  for name in "${uploads[@]}"; do patterns+=(-p "$name"); done
  "$gh" release download "$tag" -R "$repo" -D "$out/uploaded" "${patterns[@]}"
  for name in "${uploads[@]}"; do
    cmp -s "$out/up/$name" "$out/uploaded/$name" \
      || { echo "release-isos: --replace: the uploaded $name differs from the local file" >&2; replace_recovery; exit 1; }
  done
  # Ninguém trocou o SHA256SUMS desde o começo desta execução (outra máquina, ou à mão)
  mkdir "$out/recheck"
  "$gh" release download "$tag" -R "$repo" -D "$out/recheck" -p SHA256SUMS
  cmp -s "$old" "$out/recheck/SHA256SUMS" \
    || { echo "release-isos: --replace: $tag's SHA256SUMS changed during this run" >&2; replace_recovery; exit 1; }
  trap - ERR

  # As trocas, por id: NOME -> NOME.old, NOME.new -> NOME. Se a segunda falhar, o antigo volta ao nome
  local ids
  ids=$("$gh" api "repos/$repo/releases/tags/$tag" --jq '.assets[] | "\(.id) \(.name)"')
  id_of() { awk -v n="$1" '$2 == n { print $1 }' <<<"$ids"; }
  name_of() {  # o nome atual do asset $1, ou nada
    "$gh" api "repos/$repo/releases/tags/$tag" --jq '.assets[] | "\(.id) \(.name)"' 2>/dev/null \
      | awk -v id="$1" '$1 == id { print $2 }' || true
  }
  rename() { "$gh" api -X PATCH "repos/$repo/releases/assets/$1" -f name="$2" >/dev/null; }
  local swapped=() target old_id new_id now
  swap_failed() {  # $1: o que falhou; as trocas já feitas ficam (são arquivos conferidos)
    echo "release-isos: --replace: $1" >&2
    ((${#swapped[@]})) && echo "release-isos: already swapped (verified, but not yet in SHA256SUMS): ${swapped[*]}" >&2
    echo "release-isos: fix it by hand: each NAME.old is the published file before this run; SHA256SUMS.new and" >&2
    echo "release-isos: $out/SHA256SUMS.new have the new sums. List the assets with:" >&2
    echo "  $gh release view $tag -R $repo --json assets -q '.assets[].name'" >&2
    exit 1
  }
  for target in "${files[@]}" SHA256SUMS; do
    old_id=$(id_of "$target") new_id=$(id_of "$target.new")
    [[ -n $old_id && -n $new_id ]] || swap_failed "can't find the assets $target and $target.new"
    if ! rename "$old_id" "$target.old" && [[ $(name_of "$old_id") != "$target.old" ]]; then
      swap_failed "renaming $target to $target.old failed"
    fi
    if ! rename "$new_id" "$target" && [[ $(name_of "$new_id") != "$target" ]]; then
      now=$(name_of "$old_id")
      if [[ $now == "$target.old" ]] && rename "$old_id" "$target"; then
        echo "release-isos: the old $target has its name back" >&2
      else
        echo "release-isos: PUT IT BACK BY HAND: $gh api -X PATCH repos/$repo/releases/assets/$old_id -f name=$target" >&2
      fi
      swap_failed "renaming $target.new to $target failed"
    fi
    swapped+=("$target")
  done

  # Tudo no lugar: primeiro o que o GitHub serve agora confere (os .old ainda estão lá, para voltar atrás), e só
  # então os .old saem
  replace_check_failed() {
    echo "release-isos: the new files and SHA256SUMS are in place, but the check of the download failed." >&2
    echo "release-isos: the published files before this run are still there as NAME.old. To go back, for each" >&2
    echo "release-isos: of ${files[*]} SHA256SUMS: delete NAME, then rename NAME.old to NAME, e.g.:" >&2
    echo "  $gh release delete-asset $tag NAME -R $repo -y" >&2
    echo "  $gh api -X PATCH repos/$repo/releases/assets/<id of NAME.old> -f name=NAME" >&2
  }
  trap replace_check_failed ERR
  mkdir "$out/download"
  patterns=(-p SHA256SUMS)
  while IFS= read -r line; do patterns+=(-p "${line#*  }"); done < "$out/SHA256SUMS.new"
  "$gh" release download "$tag" -R "$repo" -D "$out/download" "${patterns[@]}"
  if ! cmp -s "$out/SHA256SUMS.new" "$out/download/SHA256SUMS"; then
    echo "release-isos: the downloaded SHA256SUMS differs" >&2; replace_check_failed; exit 1
  fi
  (cd "$out/download" && sha256sum -c SHA256SUMS)
  trap - ERR
  old_left() {
    echo "release-isos: the new files are published and verified; deleting $1 failed. Delete what is left:" >&2
    for name in "${files[@]}" SHA256SUMS; do
      echo "  $gh release delete-asset $tag $name.old -R $repo -y   (if it is still there)" >&2
    done
    exit 1
  }
  for name in "${files[@]}" SHA256SUMS; do
    "$gh" release delete-asset "$tag" "$name.old" -R "$repo" -y || old_left "$name.old"
  done
  echo "release-isos: $tag now has the new ${files[*]}; SHA256SUMS lists all its ISOs and they all check"
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
if ((replace)); then
  ((${#left[@]} == 0)) || { echo "release-isos: --replace: an ISO is over the limit; nothing replaced" >&2; exit 1; }
  replace_mode
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
