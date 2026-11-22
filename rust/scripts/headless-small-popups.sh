#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa os popups da cafeína, do brilho e da bateria num Sway headless: a captura (grim) e os cliques, com o
# ponteiro virtual (swaydesk-vpointer). Roda dentro do contêiner swaydesk-smoke:debian13, como o
# headless-toasts.sh, com os binários swaydesk-caffeine-popup, swaydesk-brightness-popup,
# swaydesk-battery-popup e swaydesk-vpointer em /bin.
#
# O sway-caffeine, o sway-nightlight e o sway-brightness são falsos: anotam cada chamada e guardam o
# estado num arquivo. O brilho lê uma pasta falsa (BACKLIGHT_DIR). A bateria lê um upower e um busctl
# falsos, no começo do PATH. Para cada versão (rust e python): a
# captura de cada popup e os cliques nos botões, conferidos pelas chamadas anotadas, e o popup segue
# aberto. Sai com 1 se algo falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/sway-desktop" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
# Cópias, não links: o Python chama os vizinhos pelo caminho real (o BIN do waybar_popup.py), e os
# falsos abaixo precisam estar ao lado dele
cp /repo/home/.local/bin/* "$HOME/.local/bin/"
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
calls=/tmp/calls state=/tmp/state
mkdir -p "$state"
cat > "$HOME/.local/bin/sway-caffeine" <<'SCRIPT'
#!/bin/sh
echo "sway-caffeine $*" >>/tmp/calls
case $1 in
  status) cat /tmp/state/caffeine 2>/dev/null || echo off ;;
  off) echo off >/tmp/state/caffeine ;;
  on) echo forever >/tmp/state/caffeine ;;
  1h|2h) n=${1%h}; e=$(( $(date +%s) + n * 3600 )); echo "until $e $(date -d "@$e" +%H:%M)" >/tmp/state/caffeine ;;
esac
SCRIPT
cat > "$HOME/.local/bin/sway-nightlight" <<'SCRIPT'
#!/bin/sh
case $1 in status|schedule) ;; *) echo "sway-nightlight $*" >>/tmp/calls ;; esac
case $1 in
  status) cat /tmp/state/night 2>/dev/null || echo off ;;
  schedule) if [ $# -eq 3 ]; then echo "$2 $3" >/tmp/state/schedule; else cat /tmp/state/schedule 2>/dev/null || echo "18:00 06:00"; fi ;;
  off|on|auto) echo "$1" >/tmp/state/night ;;
esac
SCRIPT
cat > "$HOME/.local/bin/sway-brightness" <<'SCRIPT'
#!/bin/sh
echo "sway-brightness $*" >>/tmp/calls
[ "$1" = set ] && echo $(( $2 * 1000 / 100 )) >/tmp/backlight/panel/brightness
SCRIPT
chmod +x "$HOME/.local/bin/sway-caffeine" "$HOME/.local/bin/sway-nightlight" "$HOME/.local/bin/sway-brightness"
mkdir -p /tmp/backlight/panel /tmp/fakebin
cat > /tmp/fakebin/upower <<'SCRIPT'
#!/bin/sh
case $1 in
  -e) printf '/org/freedesktop/UPower/devices/battery_BAT0\n/org/freedesktop/UPower/devices/line_power_AC\n/org/freedesktop/UPower/devices/mouse_dev_1\n' ;;
  -i) case $2 in
        *battery_BAT0) printf '  vendor:               SMP\n  model:                5B10W13930\n    state:               discharging\n    energy:              38.43 Wh\n    energy-full:         49.68 Wh\n    energy-full-design:  57 Wh\n    energy-rate:         7.512 W\n    time to empty:       5.1 hours\n    percentage:          77%%\n    technology:          lithium-polymer\n    charge-cycles:       120\n' ;;
        *mouse_dev_1) printf '  model:                MX Master 3\n    percentage:          55%%\n' ;;
      esac ;;
esac
SCRIPT
cat > /tmp/fakebin/busctl <<'SCRIPT'
#!/bin/sh
case "$*" in
  *get-property*) cat /tmp/state/profile 2>/dev/null || echo 's "balanced"' ;;
  *set-property*) for a; do last=$a; done; echo "busctl set-property $last" >>/tmp/calls; echo "s \"$last\"" >/tmp/state/profile ;;
  *GetHistory*) now=$(date +%s); printf '{"type":"a(udu)","data":[[[%d,95.0,2],[%d,85.0,2],[%d,77.0,2]]]}\n' $((now-36000)) $((now-18000)) $((now-600)) ;;
esac
SCRIPT
chmod +x /tmp/fakebin/upower /tmp/fakebin/busctl
export BACKLIGHT_DIR=/tmp/backlight
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
failed=0
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5

expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
click() { /bin/swaydesk-vpointer --size 1280x720 move "$1" "$2" click sleep 200; sleep 0.8; }
open_popup() {  # impl script variável
  env "$3=$1" "$HOME/.local/bin/$2" 2>>/tmp/popup.err & popup=$!
  for _ in $(seq 50); do [ -e "$XDG_RUNTIME_DIR/$2.pid" ] && break; sleep 0.1; done
  sleep 1.2
}
close_popup() { kill "$popup" 2>/dev/null; wait "$popup" 2>/dev/null; sleep 0.5; }
still_open() { expect "$1: o popup continua aberto depois dos cliques" "aberto" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"; }
last_call() { tail -n 1 "$calls" 2>/dev/null; }

for impl in rust python; do
  # --- cafeína ---
  echo off >"$state/caffeine"; : >"$calls"
  open_popup "$impl" waybar-caffeine-popup SWAY_CAFFEINE_POPUP_IMPL
  echo "$impl: cafeína $(readlink /proc/$popup/exe)"
  grim "/out/caffeine-$impl.png"
  click 1142 59
  expect "$impl: 2 h liga a cafeína" "sway-caffeine 2h" "$(grep -v status "$calls" | tail -n 1)"
  sleep 1
  grim "/out/caffeine-$impl-2h.png"
  click 1220 59
  expect "$impl: Sempre" "sway-caffeine on" "$(grep -v status "$calls" | tail -n 1)"
  still_open "$impl: cafeína"
  close_popup

  # --- brilho ---
  echo 500 >/tmp/backlight/panel/brightness; echo 1000 >/tmp/backlight/panel/max_brightness
  echo off >"$state/night"; echo "18:00 06:00" >"$state/schedule"; : >"$calls"
  open_popup "$impl" waybar-brightness-popup SWAY_BRIGHTNESS_POPUP_IMPL
  echo "$impl: brilho $(readlink /proc/$popup/exe)"
  grim "/out/brightness-$impl.png"
  click 1207 109
  expect "$impl: Programada liga a luz noturna" "sway-nightlight auto" "$(grep nightlight "$calls" | tail -n 1)"
  sleep 1
  grim "/out/brightness-$impl-auto.png"
  click 1150 53
  sleep 0.5
  expect "$impl: o slider grava o brilho" "sway-brightness set" "$(grep brightness "$calls" | tail -n 1 | cut -d' ' -f1-2)"
  still_open "$impl: brilho"
  close_popup

  # --- bateria ---
  echo 's "balanced"' >"$state/profile"; : >"$calls"
  PATH=/tmp/fakebin:$PATH open_popup "$impl" waybar-battery-popup SWAY_BATTERY_POPUP_IMPL
  echo "$impl: bateria $(readlink /proc/$popup/exe)"
  grim "/out/battery-$impl.png"
  click 1204 359
  expect "$impl: Desempenho muda o perfil" "busctl set-property performance" "$(grep set-property "$calls" | tail -n 1)"
  sleep 1
  grim "/out/battery-$impl-perf.png"
  still_open "$impl: bateria"
  close_popup
done
cat /tmp/popup.err 2>/dev/null | grep -v '^$' | head -20
exit "$failed"
