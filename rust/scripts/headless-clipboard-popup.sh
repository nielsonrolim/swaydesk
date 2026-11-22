#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o popup de clipboard num Sway headless, com um cliphist e um wl-copy de mentira: a captura (grim)
# e os cliques, com o ponteiro virtual (swaydesk-vpointer). Roda dentro do contêiner swaydesk-smoke:debian13,
# como o headless-toasts.sh, com o swaydesk-clipboard-popup e o swaydesk-vpointer em /bin.
#
# O histórico: um texto, um texto em UTF-16, uma imagem PNG e um PDF. Para cada versão (rust e python): a
# captura; o clique num item copia (o wl-copy recebe os bytes certos) e fecha; o pino fixa (o pins.json
# ganha o item); a lixeira apaga do histórico. Os fixados e as miniaturas são os mesmos arquivos nas duas
# versões, então cada uma começa do zero. Depois, com um histórico longo rolado para baixo (a roda do
# ponteiro): fixar e apagar deixam a lista onde estava, e a linha seguinte toma o lugar da que saiu.
# Sai com 1 se algo falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" /tmp/fakebin && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do [ -f "$f" ] && ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
# O histórico: um arquivo por item em /tmp/hist/<id>, e a prévia em /tmp/hist/<id>.preview
reset_history() {
  rm -rf /tmp/hist; mkdir -p /tmp/hist
  printf 'texto comum para copiar' > /tmp/hist/4; printf 'texto comum para copiar' > /tmp/hist/4.preview
  python3 -c 'import sys; sys.stdout.buffer.write("https://exemplo.com/ação".encode("utf-16-le"))' > /tmp/hist/3
  cp /tmp/hist/3 /tmp/hist/3.preview
  python3 -c '
import struct, zlib
w, h = 40, 30
rows = b"".join(b"\x00" + bytes(v for x in range(w) for v in (200, 40 + x * 5, 90)) for _ in range(h))
def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
open("/tmp/hist/2", "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))'
  printf '[[ binary data 1 KiB png 40x30 ]]' > /tmp/hist/2.preview
  printf '%%PDF-1.7\n%%\xe2\xe3\n1 0 obj\x00\x01\x02 stream' > /tmp/hist/1; printf '%%PDF-1.7 %%\xe2\xe3' > /tmp/hist/1.preview
}
cat > /tmp/fakebin/cliphist <<'SCRIPT'
#!/bin/bash
case $1 in
  list) for f in $(ls /tmp/hist | grep -v preview | sort -rn); do printf '%s\t' "$f"; cat "/tmp/hist/$f.preview"; printf '\n'; done ;;
  decode) IFS=$'\t' read -r id _; cat "/tmp/hist/$id" ;;
  delete) IFS=$'\t' read -r id _; echo "delete $id" >>/tmp/calls; rm -f "/tmp/hist/$id" "/tmp/hist/$id.preview" ;;
  wipe) echo wipe >>/tmp/calls; rm -f /tmp/hist/* ;;
esac
SCRIPT
printf '#!/bin/sh\necho "wl-copy $*" >>/tmp/calls; cat > /tmp/copied\n' > /tmp/fakebin/wl-copy
chmod +x /tmp/fakebin/*
export PATH=/tmp/fakebin:$PATH
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
click() { /bin/swaydesk-vpointer --size 1280x720 move "$1" "$2" click sleep 200; sleep 1; }
open_popup() {
  # O ponteiro longe: parado sobre uma linha, a dica dela cobriria o ponto do próximo clique
  /bin/swaydesk-vpointer --size 1280x720 move 300 600 sleep 100
  SWAY_CLIPBOARD_POPUP_IMPL=$1 "$HOME/.local/bin/waybar-clipboard-popup" 2>>/tmp/popup.err & popup=$!
  sleep 3.5
}
state() { kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado; }
for impl in ${IMPLS:-rust python}; do
  reset_history; : >/tmp/calls; rm -f /tmp/copied; rm -rf "$HOME/.local/share/waybar-clipboard" "$HOME/.cache/waybar-clipboard"
  open_popup "$impl"
  echo "$impl: popup $(readlink /proc/$popup/exe)"
  grim "/out/clip-$impl.png"
  click 1050 110  # o primeiro item: o texto comum
  expect "$impl: o clique copia o texto" "texto comum para copiar" "$(cat /tmp/copied 2>/dev/null)"
  for _ in $(seq 50); do kill -0 "$popup" 2>/dev/null || break; sleep 0.1; done  # o Python sai devagar
  expect "$impl: copiar fecha o popup" "fechado" "$(state)"
  open_popup "$impl"
  click 1050 160  # o segundo: o UTF-16, que vai convertido para UTF-8
  expect "$impl: o UTF-16 vai em UTF-8" "https://exemplo.com/ação" "$(cat /tmp/copied 2>/dev/null)"
  expect "$impl: com o tipo do UTF-8" "wl-copy --type text/plain;charset=utf-8" "$(grep wl-copy /tmp/calls | tail -n 1)"
  for _ in $(seq 50); do kill -0 "$popup" 2>/dev/null || break; sleep 0.1; done
  open_popup "$impl"
  click 1189 110  # o pino do primeiro
  sleep 0.5
  expect "$impl: o pino fixa" "1" "$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["items"]))' "$HOME/.local/share/waybar-clipboard/pins.json" 2>/dev/null)"
  grim "/out/clip-$impl-pinned.png"
  click 1226 298  # a lixeira do último recente (o PDF)
  sleep 0.5
  expect "$impl: a lixeira apaga do histórico" "delete 1" "$(grep delete /tmp/calls | tail -n 1)"
  grim "/out/clip-$impl-deleted.png"
  expect "$impl: o popup continua aberto" "aberto" "$(state)"
  kill $popup 2>/dev/null; wait $popup 2>/dev/null; sleep 0.5
done
# O histórico longo: 121 textos, o mais novo em cima
long_history() {
  rm -rf /tmp/hist; mkdir -p /tmp/hist
  for n in $(seq 121); do printf 'item número %s' "$n" | tee /tmp/hist/$n > /tmp/hist/$n.preview; done
}
# O centro da linha sob o ponteiro (em x=1050, y): o fundo de hover dela, na coluna x=945 da captura. Os
# botões são menores que a linha; um clique na borda dela cairia fora deles
row_center() {
  /bin/swaydesk-vpointer --size 1280x720 move 1050 "$1" sleep 400
  grim /tmp/hover.png
  python3 - "$1" <<'PY'
import sys
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
pb = GdkPixbuf.Pixbuf.new_from_file("/tmp/hover.png")
px, stride, n = pb.get_pixels(), pb.get_rowstride(), pb.get_n_channels()
at = lambda y: px[y * stride + 945 * n:y * stride + 945 * n + 3]
y = int(sys.argv[1]); color = at(y); top = bottom = y
while at(top - 1) == color: top -= 1
while at(bottom + 1) == color: bottom += 1
print((top + bottom) // 2)
PY
}
scrolled_popup() {  # impl: abre e rola 12 passos da roda
  open_popup "$1"
  /bin/swaydesk-vpointer --size 1280x720 move 1050 200 scroll 12 sleep 800
}
wait_closed() { for _ in $(seq 50); do kill -0 "$popup" 2>/dev/null || break; sleep 0.1; done; }
copied_at() { rm -f /tmp/copied; click 1050 "$1"; wait_closed; cat /tmp/copied 2>/dev/null; }
for impl in ${IMPLS:-rust python}; do
  long_history; : >/tmp/calls; rm -rf "$HOME/.local/share/waybar-clipboard" "$HOME/.cache/waybar-clipboard"
  scrolled_popup "$impl"; y=$(row_center 300); before=$(copied_at "$y"); n=${before##* }
  echo "$impl: rolada, a linha em y=$y é \"$before\""
  [ -n "$n" ] && [ "$n" -lt 110 ] || { echo "FALHOU  $impl: a roda não rolou a lista"; failed=1; continue; }
  # Fixar o item n: ele vai para Fixados, e a linha seguinte (n-1) fica na mesma altura
  scrolled_popup "$impl"; y=$(row_center 300)
  /bin/swaydesk-vpointer --size 1280x720 move 1189 "$y" click sleep 1500
  grim "/out/clip-$impl-long-pinned.png"
  expect "$impl: fixar com a lista rolada não a move" "item número $((n - 1))" "$(copied_at "$y")"
  # Apagar a linha: a seguinte sobe para o lugar
  scrolled_popup "$impl"; y=$(row_center 300)
  /bin/swaydesk-vpointer --size 1280x720 move 1226 "$y" click sleep 1500
  deleted=$(grep delete /tmp/calls | tail -n 1); deleted=${deleted#delete }
  grim "/out/clip-$impl-long-deleted.png"
  expect "$impl: apagar com a lista rolada não a move" "item número $((deleted - 1))" "$(copied_at "$y")"
done
grep -v '^$' /tmp/popup.err | grep -v 'session bus\|a11y\|AT-SPI' | head -10
exit "$failed"
