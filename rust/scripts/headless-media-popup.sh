#!/bin/bash
# Testa o popup de mídia num Sway headless, com players MPRIS de mentira (tests/fakes/mpris-player) num
# barramento só do teste: a captura (grim) e os cliques, com o ponteiro virtual (swaydesk-vpointer). Roda
# dentro do contêiner swaydesk-smoke:debian13, como o headless-toasts.sh, com o swaydesk-media-popup e o
# swaydesk-vpointer em /bin.
#
# Para cada versão (rust e python): um player tocando, com capa (um PNG local), e outro pausado; a
# captura, o play/pause e o "próxima" (conferidos pelas chamadas que o player falso anota), a troca para o
# outro player e a captura. Sai com 1 se algo falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/sway-desktop" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do [ -f "$f" ] && ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
# Uma capa quadrada de 300 × 300 (PNG de verdade)
python3 -c '
import struct, zlib
w = h = 300
row = bytes(v for x in range(w) for v in (30 + (x * 200) // w, 90, 160 - (x * 100) // w))
rows = b"".join(b"\x00" + row for _ in range(h))
def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
open("/tmp/capa.png", "wb").write(png)'
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
DBUS_SESSION_BUS_ADDRESS=$(dbus-daemon --session --fork --print-address=1)
export DBUS_SESSION_BUS_ADDRESS
failed=0
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5
# Os players falsos: comandos em JSON por um FIFO, chamadas anotadas num arquivo
start_player() {  # sufixo identidade
  mkfifo "/tmp/in.$1"
  /repo/tests/fakes/mpris-player "$DBUS_SESSION_BUS_ADDRESS" "$1" "$2" <"/tmp/in.$1" >"/tmp/out.$1" &
  eval "exec {fd_$1}>/tmp/in.$1"
  for _ in $(seq 50); do grep -q ready "/tmp/out.$1" 2>/dev/null && break; sleep 0.1; done
}
say() { local fd="fd_$1"; printf '%s\n' "$2" >&"${!fd}"; sleep 0.3; }
start_player um "Tocador Um"
start_player dois "Tocador Dois"
say um '{"status": "Playing", "meta": {"xesam:title": "Canção de Teste", "xesam:artist": ["Artista"], "xesam:album": "Disco", "mpris:length": 200000000, "mpris:trackid": "/faixa/1", "mpris:artUrl": "file:///tmp/capa.png"}, "props": {"Position": 65000000}}'
say dois '{"status": "Paused", "meta": {"xesam:title": "Outra faixa", "xesam:artist": ["Banda"], "mpris:length": 100000000}}'

expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
click() { /bin/swaydesk-vpointer --size 1280x720 move "$1" "$2" click sleep 200; sleep 0.8; }
calls() { grep -cx "$2" "/tmp/out.$1"; }
for impl in rust python; do
  SWAY_MEDIA_POPUP_IMPL=$impl "$HOME/.local/bin/waybar-media-popup" 2>>/tmp/popup.err & popup=$!
  sleep 2.5
  echo "$impl: popup $(readlink /proc/$popup/exe)"
  grim "/out/media-$impl.png"
  before=$(calls um PlayPause)
  click 1093 398
  expect "$impl: play/pause chega ao player" "$((before + 1))" "$(calls um PlayPause)"
  before=$(calls um Next)
  click 1149 398
  expect "$impl: próxima chega ao player" "$((before + 1))" "$(calls um Next)"
  click 1010 34
  sleep 0.5
  grim "/out/media-$impl-other.png"
  expect "$impl: o popup continua aberto" "aberto" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"
  kill $popup 2>/dev/null; wait $popup 2>/dev/null; sleep 0.5
done
grep -v '^$' /tmp/popup.err | grep -v 'session bus' | head -10
exit "$failed"
