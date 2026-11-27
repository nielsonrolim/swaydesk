#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa a cafeína fora da sessão do Swaydesk num Sway headless com dois monitores e um swayidle de verdade (o do
# "usuário", com timeout 3). Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório em /repo e os
# binários em /bin:
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-caffeinate:/bin/swaydesk-caffeinate:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     swaydesk-smoke:debian13 /repo/rust/scripts/headless-caffeinate.sh
#
# - sem a cafeína, o swayidle dispara (o controle);
# - sway-caffeine on (sem SWAYDESK_SESSION): o swaydesk-caffeinate sobe como sway-caffeinate, um só mesmo
#   ligando de novo, e o swayidle não dispara;
# - um monitor desativado e religado ganha a superfície de volta: desativado o outro, ainda não dispara;
# - sway-caffeine off: o inibidor sai e o swayidle volta a disparar;
# - o swayidle é o mesmo processo do começo ao fim; com o fim do Sway, o sway-caffeinate sai.
# Sai com 1 se algo falhar.
set -u
. /repo/rust/scripts/lib-wait.sh
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
cp /repo/home/.local/bin/sway-caffeine /repo/home/.local/bin/sway-i18n.sh "$HOME/.local/bin/"
printf '#!/bin/sh\nexit 0\n' >"$HOME/.local/bin/waybar-notify"
chmod +x "$HOME/.local/bin/waybar-notify"
unset SWAYDESK_SESSION
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=2 LANG=C.UTF-8
printf 'output * resolution 1280x720 bg #000000 solid_color\n' >/tmp/sway.conf
sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
sway_pid=$!
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
caffeine=$HOME/.local/bin/sway-caffeine

failed=0
expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
# Atividade (o ponteiro virtual) e depois 6 s parado: quantas vezes o swayidle disparou
idle_fires() {
  /bin/swaydesk-vpointer --size 1280x720 move 20 20 move 30 30
  : >/tmp/fires
  sleep 6
  wc -l </tmp/fires
}
holders() { pgrep -c -u "$(id -u)" -f '^sway-caffeinate$'; }

swayidle -w timeout 3 'echo fire >>/tmp/fires' >/tmp/swayidle.log 2>&1 &
idle_pid=$!
sleep 0.5
expect "sem a cafeína, o swayidle dispara" 1 "$(idle_fires)"

"$caffeine" on
expect "on: o estado" forever "$("$caffeine" status)"
pid=$(cat "$XDG_RUNTIME_DIR/sway-caffeinate.pid" 2>/dev/null)
expect "on: o argv[0] vira sway-caffeinate" sway-caffeinate "$(tr '\0' '\n' </proc/"${pid:-0}"/cmdline 2>/dev/null | head -n 1)"
"$caffeine" on
"$caffeine" 2h
expect "ligar de novo não abre outro" 1 "$(holders)"
expect "com a cafeína, o swayidle não dispara" 0 "$(idle_fires)"

swaymsg -q output HEADLESS-1 disable
sleep 0.5
expect "um monitor desativado: o outro ainda segura" 0 "$(idle_fires)"
swaymsg -q output HEADLESS-1 enable
sleep 0.5
swaymsg -q output HEADLESS-2 disable
sleep 0.5
expect "o monitor religado ganhou a superfície de volta" 0 "$(idle_fires)"
swaymsg -q output HEADLESS-2 enable
expect "o sway-caffeinate seguiu no ar" 1 "$(holders)"

"$caffeine" off
for _ in $(seq $ticks); do [ "$(holders)" = 0 ] && break; sleep 0.1; done
expect "off: o sway-caffeinate sai" 0 "$(holders)"
expect "off: o arquivo de PID some" no "$([ -e "$XDG_RUNTIME_DIR/sway-caffeinate.pid" ] && echo yes || echo no)"
expect "off: o swayidle volta a disparar" 1 "$(idle_fires)"
expect "o swayidle é o mesmo processo" yes "$(kill -0 "$idle_pid" 2>/dev/null && echo yes || echo no)"

"$caffeine" on
expect "ligada de novo" 1 "$(holders)"
kill "$idle_pid" 2>/dev/null
kill "$sway_pid"
for _ in $(seq $ticks); do [ "$(holders)" = 0 ] && break; sleep 0.1; done
expect "com o fim do Sway, o sway-caffeinate sai" 0 "$(holders)"
if [ "$failed" != 0 ]; then
  echo "--- sway.log"; tail -n 30 /tmp/sway.log
  echo "--- swayidle.log"; cat /tmp/swayidle.log
fi
exit "$failed"
