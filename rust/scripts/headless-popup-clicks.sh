#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o clique fora e o clique dentro dos popups num Sway headless, com o ponteiro virtual
# (swaydesk-vpointer): no headless não há dispositivo, e o `swaymsg seat … cursor press` não chega
# a nenhum cliente. Roda dentro do contêiner swaydesk-smoke:debian13 (experiments/bench), com o
# repositório em /repo e os binários em /bin:
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) --group-add $(getent group render | cut -d: -f3) --device /dev/dri \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-calendar:/bin/swaydesk-calendar:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     -v /usr/share/icons:/usr/share/icons:ro swaydesk-smoke:debian13 /repo/rust/scripts/headless-popup-clicks.sh
#
# Cada caso: abre o popup (Rust e Python), espera o pidfile, clica e confere se ele fechou ou continua.
# Sai com 1 se algum caso falhar. Para outro popup, acrescente linhas no fim.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
failed=0
# Um ponteiro virtual vivo o tempo todo: o seat já tem ponteiro quando cada popup sobe, e o GTK cria o
# wl_pointer ao abrir. Os cliques de cada caso só mandam eventos (sem corrida com a espera de 300 ms)
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5

# check PROGRAMA VARIÁVEL_IMPL IMPL X Y ESPERADO(aberto|fechado)
check() {
  local program=$1 var=$2 impl=$3 x=$4 y=$5 expected=$6 pid got
  env "$var=$impl" "$HOME/.local/bin/$program" 2>/dev/null & pid=$!
  for _ in $(seq 50); do [ -e "$XDG_RUNTIME_DIR/$program.pid" ] && break; sleep 0.1; done
  sleep 1  # o popup aparece e os catchers mapeiam
  # Um popup que nem abriu (caiu na subida) passaria no caso "fechado" sem ter testado nada
  if [ ! -e "$XDG_RUNTIME_DIR/$program.pid" ] || ! kill -0 "$pid" 2>/dev/null; then
    printf 'FALHOU  %s (%s): não abriu\n' "$program" "$impl"
    failed=1
    wait "$pid" 2>/dev/null
    return
  fi
  /bin/swaydesk-vpointer --size 1280x720 move "$x" "$y" click sleep 200
  sleep 0.8
  if kill -0 "$pid" 2>/dev/null; then got=aberto; kill "$pid"; wait "$pid" 2>/dev/null; else got=fechado; fi
  if [ "$got" = "$expected" ]; then
    printf 'ok      %s (%s) clique em %s,%s: %s\n' "$program" "$impl" "$x" "$y" "$got"
  else
    printf 'FALHOU  %s (%s) clique em %s,%s: %s, esperado %s\n' "$program" "$impl" "$x" "$y" "$got" "$expected"
    failed=1
  fi
  sleep 0.5
}

# toggle_log IMPL: abrir de novo fecha o aberto, e o log diz por quê (o SIGTERM não passa pelo close do popup)
toggle_log() {
  local impl=$1 pid log=$XDG_RUNTIME_DIR/waybar-popups.log
  : > "$log"
  SWAY_CALENDAR_IMPL=$impl "$HOME/.local/bin/waybar-calendar" 2>/dev/null & pid=$!
  for _ in $(seq 50); do [ -e "$XDG_RUNTIME_DIR/waybar-calendar.pid" ] && break; sleep 0.1; done
  sleep 1
  SWAY_CALENDAR_IMPL=$impl "$HOME/.local/bin/waybar-calendar" 2>/dev/null
  for _ in $(seq 30); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
  if kill -0 "$pid" 2>/dev/null; then
    printf 'FALHOU  waybar-calendar (%s): abrir de novo não fechou\n' "$impl"; failed=1; kill "$pid"
  elif grep -q 'waybar-calendar: opened again: SIGTERM to the open one' "$log" && grep -q 'waybar-calendar: SIGTERM$' "$log"; then
    printf 'ok      waybar-calendar (%s): abrir de novo fecha, e o log diz por quê\n' "$impl"
  else
    printf 'FALHOU  waybar-calendar (%s): o log do toggle: %s\n' "$impl" "$(tr '\n' '|' < "$log")"; failed=1
  fi
  wait "$pid" 2>/dev/null
  sleep 0.5
}

for impl in rust python; do
  check waybar-calendar SWAY_CALENDAR_IMPL "$impl" 100 600 fechado  # fora: o catcher
  check waybar-calendar SWAY_CALENDAR_IMPL "$impl" 640 120 aberto   # dentro do popup
  toggle_log "$impl"
done
exit "$failed"
