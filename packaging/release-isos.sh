#!/usr/bin/env bash
# Anexa as ISOs de um lançamento ao Release vX.Y.Z do espelho público (github.com/nielsonrolim/swaydesk), com um
# SHA256SUMS (só SHA-256, sem assinatura GPG) e uma seção "ISOs" no fim do texto do Release. O SHA256SUMS no
# mesmo Release protege contra um download corrompido ou cortado, não contra quem controla o Release: não é
# uma assinatura (decisão do usuário: sem GPG).
#
# Cada ISO entra com o nome swaydesk-X.Y.Z-<distro>.iso (debian13-amd64, ubuntu26.04-amd64, arch-x86_64). O
# GitHub não aceita um arquivo de Release com mais de 2 GiB: uma ISO acima disso NÃO é publicada (nem aqui nem
# na página de download do site; decisão do usuário: nada de dividir, enxugar ou outro servidor). O script
# avisa qual ficou de fora e com que tamanho, e segue com as outras.
#
# Depois do envio, baixa do Release o que acabou de subir e confere com o SHA256SUMS: o que o usuário baixa é
# o que foi conferido aqui. Um Release que já tem um desses arquivos não é sobrescrito (nada de --clobber).
#
# uso: packaging/release-isos.sh [--dry-run] vX.Y.Z PASTA debian13=ISO ubuntu2604=ISO arch=ISO
#      PASTA recebe os nomes finais (links físicos ou cópias), o SHA256SUMS e a conferência do download: fica
#      fora do /tmp, porque são gigabytes. Com --dry-run, monta e confere a PASTA e mostra o que enviaria.
# GH troca o comando gh (testes); ISO_REPO troca o espelho; ISO_LIMIT troca o limite em bytes (testes).
set -euo pipefail
gh=${GH:-gh}
repo=${ISO_REPO:-nielsonrolim/swaydesk}
limit=${ISO_LIMIT:-2147483648}  # 2 GiB: o GitHub só aceita um arquivo de Release abaixo disso

dry=0
[[ ${1:-} == --dry-run ]] && { dry=1; shift; }
usage() { echo "uso: packaging/release-isos.sh [--dry-run] vX.Y.Z PASTA debian13=ISO ubuntu2604=ISO arch=ISO" >&2; exit 2; }
(($# >= 3)) || usage
tag=$1 out=$2
shift 2
[[ $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]] || { echo "release-isos: the tag must be vX.Y.Z, not $tag" >&2; exit 2; }
version=${BASH_REMATCH[1]}

declare -A suffix=([debian13]=debian13-amd64 [ubuntu2604]=ubuntu26.04-amd64 [arch]=arch-x86_64)
declare -A label=([debian13]="Debian 13" [ubuntu2604]="Ubuntu 26.04" [arch]="Arch Linux")
order=(debian13 ubuntu2604 arch)
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

for name in "${suffix[@]}"; do
  name=swaydesk-$version-$name.iso
  if grep -qxF "$name" <<<"$assets"; then
    echo "release-isos: $tag already has $name; nothing sent" >&2; exit 1
  fi
done
if grep -qxF SHA256SUMS <<<"$assets"; then
  echo "release-isos: $tag already has SHA256SUMS; nothing sent" >&2; exit 1
fi

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
