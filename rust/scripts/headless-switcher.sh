#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o seletor de janelas (sway-switcher) num Sway headless. Roda dentro do contêiner swaydesk-smoke:debian13,
# como o headless-osd-bar.sh, com o swaydesk-switcher e o swaydesk-vpointer em /bin; as capturas vão para /out.
#
# Para cada versão (rust e python), com três janelas de teste (A e B no workspace 1, C no 2):
# - o residente sobe, grava o PID e acompanha a ordem de uso pelos eventos do Sway;
# - o next abre o cartão com a janela usada antes selecionada (captura); o clique no meio do cartão (a
#   segunda linha de três) foca essa janela, e o Sway troca de workspace se preciso;
# - a ordem segue o foco: o next seguinte seleciona a outra;
# - a cor de destaque trocada com o residente no ar vale na abertura seguinte (o tema não o reinicia);
# - sem o teclado (o headless não tem), o cartão fecha sozinho no prazo, sem trocar o foco;
# - sem o residente, o next anda pela árvore.
# O teclado (soltar o Alt, o toque rápido, Tab e Esc) fica para as VMs: o Sway headless não tem teclado.
# Sai com 1 se algo falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
cp /repo/home/.local/bin/* "$HOME/.local/bin/"
cp /bin/swaydesk-switcher "$HOME/.local/bin/"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720 bg #000000 solid_color\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
failed=0
expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
focused() {
  swaymsg -t get_tree | python3 -c '
import json, sys
def walk(n):
    if n.get("focused") and n.get("pid"): print(n.get("name")); return True
    return any(walk(c) for c in n.get("nodes", []) + n.get("floating_nodes", []))
walk(json.load(sys.stdin))'
}
# Quantos pixels da cor $1 (#rrggbb) há na faixa do meio do cartão (a linha selecionada)
accent_pixels() {
  grim -g "340,340 600x40" -t ppm - | python3 -c '
import sys
data = sys.stdin.buffer.read()
# P6, largura, altura, máximo e os pixels
parts = data.split(maxsplit=4)
pixels = parts[4]
want = bytes.fromhex(sys.argv[1].lstrip("#"))
print(sum(1 for i in range(0, len(pixels) - 2, 3) if pixels[i:i + 3] == want))' "$1"
}
# Uma janela GTK de teste com título e app_id próprios
open_window() {
  python3 - "$1" <<'PY' &
import sys
import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk
GLib.set_prgname(f"teste-{sys.argv[1].lower()}")
w = Gtk.Window(title=sys.argv[1])
w.connect("destroy", Gtk.main_quit)
w.show_all()
Gtk.main()
PY
  for _ in $(seq 50); do [ "$(focused)" = "$1" ] && return; sleep 0.1; done
}
open_window A
open_window B
swaymsg -q workspace 2
open_window C
for impl in ${IMPLS:-rust python}; do
  rm -f "$XDG_RUNTIME_DIR"/sway-switcher.*
  # O cartão aparece logo (50 ms) e espera o clique (4 s) antes de fechar por falta de teclado
  SWAY_SWITCHER_IMPL=$impl SWAY_SWITCHER_SHOW_MS=50 SWAY_SWITCHER_FOCUS_MS=4000 \
    "$HOME/.local/bin/sway-switcher" daemon 2>/tmp/switcher-$impl.log & daemon=$!
  for _ in $(seq 100); do [ -s "$XDG_RUNTIME_DIR/sway-switcher.pid" ] && break; sleep 0.1; done
  sleep 0.5
  echo "$impl: $(readlink /proc/$daemon/exe), comm $(cat /proc/$daemon/comm)"
  expect "$impl: o PID do arquivo é o do residente" "$daemon" "$(cat "$XDG_RUNTIME_DIR/sway-switcher.pid" 2>/dev/null)"
  # A ordem de uso: A, depois B, depois C (o foco atual)
  for name in A B C; do swaymsg -q "[title=\"^$name\$\"] focus"; sleep 0.2; done
  expect "$impl: o foco antes" "C" "$(focused)"
  SWAY_SWITCHER_IMPL=$impl "$HOME/.local/bin/sway-switcher" next
  sleep 0.5
  grim "/out/switcher-$impl-1.png"
  # Três linhas: o meio do cartão é a segunda, a janela usada antes (B)
  /bin/swaydesk-vpointer --size 1280x720 move 640 360 click sleep 300
  expect "$impl: o clique foca a usada antes" "B" "$(focused)"
  # A cor de destaque muda com o residente no ar: a próxima abertura já usa a nova
  mkdir -p "$HOME/.config/sway-desktop"
  printf 'accent = "#c03030"\n' > "$HOME/.config/sway-desktop/config.toml"
  SWAY_SWITCHER_IMPL=$impl "$HOME/.local/bin/sway-switcher" next
  sleep 0.5
  grim "/out/switcher-$impl-2.png"
  n=$(accent_pixels '#c03030')
  expect "$impl: a cor de destaque nova sem reiniciar" yes "$([ "${n:-0}" -gt 100 ] && echo yes || echo "no ($n px)")"
  rm -f "$HOME/.config/sway-desktop/config.toml"
  /bin/swaydesk-vpointer --size 1280x720 move 640 360 click sleep 300
  expect "$impl: a ordem segue o foco (de B volta para C)" "C" "$(focused)"
  # Sem o teclado e sem clique, o cartão fecha no prazo e o foco fica
  SWAY_SWITCHER_IMPL=$impl "$HOME/.local/bin/sway-switcher" prev
  sleep 4.5
  expect "$impl: sem o teclado, fecha sem trocar" "C" "$(focused)"
  grim "/out/switcher-$impl-closed.png"
  kill $daemon 2>/dev/null; wait $daemon 2>/dev/null
  expect "$impl: o PID sai junto" "" "$(cat "$XDG_RUNTIME_DIR/sway-switcher.pid" 2>/dev/null)"
  cp /tmp/switcher-$impl.log /out/
  # Sem o residente: a seguinte na árvore (de C, a primeira, A)
  SWAY_SWITCHER_IMPL=$impl "$HOME/.local/bin/sway-switcher" next
  sleep 0.3
  expect "$impl: sem o residente, anda pela árvore" "A" "$(focused)"
  swaymsg -q '[title="^C$"] focus'
done
exit "$failed"
