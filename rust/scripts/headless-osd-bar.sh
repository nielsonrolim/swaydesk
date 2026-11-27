#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o indicador de volume e brilho num Sway headless. Roda dentro do contêiner swaydesk-smoke:debian13,
# como o headless-toasts.sh, com o swaydesk-osd-bar em /bin.
#
# Para cada versão (rust e python), com o sway-osd de verdade e um wpctl falso:
# - o sway-osd acha o indicador pelo arquivo de PID e pelo /proc (o Rust roda com "sway-osd-bar" puro no
#   argv[0]) e o faz aparecer;
# - a captura mostra o cartão acima da dock, e ele some depois de 1 s;
# - os cliques passam para a janela embaixo, nas duas vezes que ele aparece (uma janela GTK de teste
#   anota cada clique), e o WAYLAND_DEBUG nunca mostra a região inteira (set_input_region(nil)).
#   O Python precisa do python3-cairo para a região, que o contêiner não tem: nele, só o Rust é conferido
#   nesse ponto.
# Depois, o Rust aberto como /bin/swaydesk-osd-bar (o nome do pacote): o sway-osd ainda o acha.
# Sai com 1 se algo falhar.
set -u
. /repo/rust/scripts/lib-wait.sh
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" /tmp/fakebin && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
cp /repo/home/.local/bin/* "$HOME/.local/bin/"
cp /bin/swaydesk-osd-bar "$HOME/.local/bin/"
cat > /tmp/fakebin/wpctl <<'SCRIPT'
#!/bin/sh
case $1 in get-volume) cat /tmp/volume ;; esac
SCRIPT
chmod +x /tmp/fakebin/wpctl
export PATH=/tmp/fakebin:$PATH
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720 bg #000000 solid_color\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
# A janela embaixo do indicador: anota cada clique
python3 - <<'PY' & under=$!
import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk
w = Gtk.Window(title="embaixo")
w.connect("button-press-event", lambda *_a: open("/tmp/clicks", "a").write("clique\n") and False)
w.connect("destroy", Gtk.main_quit)
w.show_all()
Gtk.main()
PY
sleep 2
swaymsg -q '[title="embaixo"] fullscreen enable'
sleep 0.5
failed=0
expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
# A cor de um ponto do fundo do cartão (perto da borda esquerda, sem o ícone nem a barra), acima da dock
card_pixel() {
  grim /tmp/shot.png
  python3 - <<'PY'
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
pb = GdkPixbuf.Pixbuf.new_from_file("/tmp/shot.png")
px, stride, n = pb.get_pixels(), pb.get_rowstride(), pb.get_n_channels()
i = 610 * stride + 488 * n
print("%02x%02x%02x" % tuple(px[i:i + 3]))
PY
}
# Clica no meio do cartão: com a região vazia, o clique chega à janela embaixo
click_through() {
  [ "$impl" = rust ] || return 0
  : >/tmp/clicks
  /bin/swaydesk-vpointer --size 1280x720 move 640 610 click sleep 200
  expect "$impl: o clique passa (vez $1)" "clique" "$(head -n 1 /tmp/clicks)"
}
for impl in ${IMPLS:-rust python}; do
  rm -f "$XDG_RUNTIME_DIR"/sway-osd-bar.*
  SWAY_OSD_BAR_IMPL=$impl WAYLAND_DEBUG=1 "$HOME/.local/bin/sway-osd-bar" 2>/tmp/wl-$impl.log & bar=$!
  for _ in $(seq $ticks); do [ -s "$XDG_RUNTIME_DIR/sway-osd-bar.pid" ] && break; sleep 0.1; done
  sleep 0.5
  echo "$impl: $(readlink /proc/$bar/exe), comm $(cat /proc/$bar/comm)"
  expect "$impl: o PID do arquivo é o do indicador" "$bar" "$(cat "$XDG_RUNTIME_DIR/sway-osd-bar.pid" 2>/dev/null)"
  hidden=$(card_pixel)
  echo "$impl: sem o indicador, o ponto é $hidden"
  echo 'Volume: 0.70' > /tmp/volume
  "$HOME/.local/bin/sway-osd" volume up
  sleep 0.4
  grim "/out/osd-$impl-volume.png"
  shown=$(card_pixel)
  [ "$shown" != "$hidden" ] && echo "ok      $impl: o sway-osd o faz aparecer ($shown)" || { echo "FALHOU  $impl: não apareceu"; failed=1; }
  click_through 1
  expect "$impl: o estado gravado" "volume 70" "$(cat "$XDG_RUNTIME_DIR/sway-osd-bar.state")"
  sleep 1.3
  expect "$impl: some depois de 1 s" "$hidden" "$(card_pixel)"
  echo 'Volume: 0.33 [MUTED]' > /tmp/volume
  "$HOME/.local/bin/sway-osd" volume mute
  sleep 0.4
  grim "/out/osd-$impl-muted.png"
  [ "$(card_pixel)" != "$hidden" ] && echo "ok      $impl: aparece de novo" || { echo "FALHOU  $impl: não apareceu de novo"; failed=1; }
  click_through 2
  sleep 1.3
  kill $bar 2>/dev/null; wait $bar 2>/dev/null
  log=/tmp/wl-$impl.log; cp "$log" /out/
  expect "$impl: duas superfícies (uma por vez que apareceu)" "2" "$(grep -c '\.get_layer_surface(' "$log")"
  [ "$impl" = rust ] && expect "$impl: nunca a região inteira" "0" "$(grep -c 'set_input_region(nil)' "$log")"
  expect "$impl: o PID sai junto" "" "$(cat "$XDG_RUNTIME_DIR/sway-osd-bar.pid" 2>/dev/null)"
done
# Aberto pelo nome do pacote (o PATH, um `exec swaydesk-osd-bar` num Sway puro): ele roda de novo como
# sway-osd-bar, e o sway-osd, que confere o cmdline, ainda o acha
rm -f "$XDG_RUNTIME_DIR"/sway-osd-bar.*
/bin/swaydesk-osd-bar 2>/tmp/wl-package-name.log & bar=$!
for _ in $(seq $ticks); do [ -s "$XDG_RUNTIME_DIR/sway-osd-bar.pid" ] && break; sleep 0.1; done
sleep 0.5
expect "swaydesk-osd-bar: o argv[0] vira sway-osd-bar" "sway-osd-bar" "$(tr '\0' '\n' </proc/$bar/cmdline | head -n 1)"
hidden=$(card_pixel)
echo 'Volume: 0.40' > /tmp/volume
"$HOME/.local/bin/sway-osd" volume up
sleep 0.4
expect "swaydesk-osd-bar: o sway-osd grava o estado" "volume 40" "$(cat "$XDG_RUNTIME_DIR/sway-osd-bar.state" 2>/dev/null)"
[ "$(card_pixel)" != "$hidden" ] && echo "ok      swaydesk-osd-bar: o sway-osd o faz aparecer" || { echo "FALHOU  swaydesk-osd-bar: não apareceu"; failed=1; }
kill $bar 2>/dev/null; wait $bar 2>/dev/null
kill $under 2>/dev/null
exit "$failed"
