#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# O swaydesk-language e o waybar-language (Python) num Sway headless de verdade. Roda dentro do contêiner
# swaydesk-smoke:debian13, com o repositório em /repo e o binário em /bin:
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) -v $PWD:/repo:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-language:/bin/swaydesk-language:ro \
#     swaydesk-smoke:debian13 /repo/rust/scripts/headless-language.sh
#
# O headless não tem teclado de verdade, então os dois layouts ficam para o tests/test_language.py (um Sway falso) e
# para a VM. Aqui, o que enganava o sway/language da Waybar: um "swaymsg reload" e um teclado virtual do wtype (que
# o Sway lista com layout nulo) não podem fazer o módulo aparecer: a única linha é a do começo, com o texto vazio.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\ninput type:keyboard xkb_layout br\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1
SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock 2>/dev/null | head -1)
export SWAYSOCK
failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }

check() {  # check <nome> <comando...>
  local name=$1; shift
  local out=/tmp/out-$name
  "$@" > "$out" 2>/tmp/err-$name &
  local pid=$!
  sleep 1
  swaymsg reload > /dev/null; sleep 1
  wtype -s 1500 x & local typer=$!
  sleep 0.7
  if swaymsg -r -t get_inputs | grep -q virtual_keyboard; then ok "$name: o Sway lista o teclado virtual"; else fail "$name: o wtype não criou o teclado virtual"; fi
  wait "$typer"
  swaymsg reload > /dev/null; sleep 1
  local lines; lines=$(wc -l < "$out")
  if [ "$lines" = 1 ] && grep -q '"text": *""' "$out"; then
    ok "$name: uma linha só, com o texto vazio, depois de dois reloads e do teclado virtual"
  else
    fail "$name: saída: $(tr '\n' '|' < "$out") erro: $(tr '\n' '|' < /tmp/err-$name)"
  fi
  if kill -0 "$pid" 2>/dev/null; then ok "$name: continua rodando"; else fail "$name: saiu"; fi
  kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
}

check rust /bin/swaydesk-language
check python env SWAY_LANGUAGE_IMPL=python PYTHONPATH=/repo/home/.local/bin python3 /repo/home/.local/bin/waybar-language

swaymsg exit > /dev/null 2>&1
exit "$failed"
