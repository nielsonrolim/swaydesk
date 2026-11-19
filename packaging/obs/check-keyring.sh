#!/usr/bin/env bash
# Confere que os .deb do swaydesk-archive-keyring publicados no OBS têm o sha256 do keyring/published.sha256,
# o mesmo que os READMEs públicos e a página de instalação do site mandam conferir. Rode depois de qualquer
# coisa que faça o OBS recompilar o keyring (envio, rebuild manual, mudança no prjconf ou no meta): se falhar,
# a instalação pelo README está quebrada até o hash novo entrar nos READMEs e no site.
# uso: packaging/obs/check-keyring.sh
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
base=https://download.opensuse.org/repositories/home:/nielsonrolim
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0
while read -r sum path; do
  [[ -n $sum ]] || continue
  mkdir -p "$work/$(dirname "$path")"
  if ! curl -fsSL "$base/$path" -o "$work/$path"; then
    echo "check-keyring: $path is not published (a new version? update keyring/published.sha256 and the READMEs)" >&2
    fail=1; continue
  fi
  got=$(sha256sum "$work/$path" | cut -d' ' -f1)
  if [[ $got == "$sum" ]]; then echo "OK  $path"
  else echo "check-keyring: $path changed: published $got, README $sum" >&2; fail=1; fi
done < "$here/keyring/published.sha256"
exit "$fail"
