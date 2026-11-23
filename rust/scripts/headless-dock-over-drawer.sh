#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa a dock com a grade de apps aberta (Rust ou Python), num Sway headless com o ponteiro virtual
# (swaydesk-vpointer). Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório em /repo, os
# binários em /bin e uma pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) --group-add $(getent group render | cut -d: -f3) --device /dev/dri \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-dock:/bin/swaydesk-dock:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-drawer:/bin/swaydesk-drawer:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     -v /usr/share/icons:/usr/share/icons:ro -v /tmp/out:/out swaydesk-smoke:debian13 \
#     /repo/rust/scripts/headless-dock-over-drawer.sh [rust|python]
#
# A grade cobre a tela inteira, mas a dock à vista fica fora da área de cliques dela (sway-dock.area). Os
# casos: com a grade aberta, um clique no item da dock abre o app e esconde a grade; um clique no fundo
# continua só fechando a grade; o botão direito na dock esconde a grade e mostra o menu da dock; com uma
# segunda saída, a grade aberta nela deixa livre a dock dela, e não a da outra; com a dock escondida (uma
# janela por cima), a faixa da borda fica livre e traz a dock por cima da grade. A versão
# em Python precisa do python3-gi-cairo; sem ele, o caso é pulado. Sai com 1 se algum caso falhar.
set -u
impl=${1:-rust}
if [ "$impl" = python ] && ! python3 -c 'import cairo, gi; gi.require_foreign("cairo")' 2>/dev/null; then
  echo "pulado  python: sem o python3-gi-cairo"
  exit 0
fi
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" "$HOME/.local/share/applications" \
  "$HOME/.local/share/sway-dock" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
# O app fixado só deixa um rastro: o caso é "a dock recebeu o clique", e não o app em si
cat > "$HOME/.local/share/applications/dock-test-app.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Dock test app
Exec=touch /tmp/launched
Icon=utilities-terminal
DESKTOP
echo dock-test-app > "$HOME/.local/share/sway-dock/pinned"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
pointer() { /bin/swaydesk-vpointer --size 1280x720 "$@"; }
shown() { [ -e "$XDG_RUNTIME_DIR/sway-drawer.shown" ]; }
open_grid() {
  SWAY_DRAWER_IMPL=$impl "$HOME/.local/bin/sway-drawer"
  for _ in $(seq 30); do shown && break; sleep 0.1; done
  sleep 1  # mapeada, com a região de cliques aplicada
}

SWAY_DOCK_IMPL=$impl "$HOME/.local/bin/sway-dock" > /tmp/dock.log 2>&1 &
SWAY_DRAWER_IMPL=$impl "$HOME/.local/bin/sway-drawer" start > /tmp/drawer.log 2>&1 &
for _ in $(seq 50); do [ -e "$XDG_RUNTIME_DIR/sway-dock.area" ] && break; sleep 0.1; done
sleep 1.5
# Uma dock 1280x720: o item fixado é o primeiro, à esquerda do botão da grade
read -r ax ay aw ah < "$XDG_RUNTIME_DIR/sway-dock.area" 2>/dev/null
if [ -n "${ah:-}" ]; then ok "a dock publica a área ($ax $ay $aw $ah)"; else fail "sem a área da dock"; exit 1; fi
item_x=612 item_y=688

open_grid
shown && ok "a grade abre e avisa" || fail "a grade não criou o sway-drawer.shown"
grim "/out/$impl-1-grade.png" 2>/dev/null
pointer move $item_x $item_y sleep 300 click sleep 500
sleep 1
grim "/out/$impl-2-clique.png" 2>/dev/null
[ -e /tmp/launched ] && ok "o clique na dock abre o app" || fail "o clique na dock não chegou a ela"
shown && fail "a grade continuou aberta" || ok "a grade se esconde"

open_grid
pointer move 100 680 sleep 300 click sleep 500
sleep 0.8
shown && fail "o clique no fundo não fechou a grade" || ok "o clique no fundo fecha a grade"

open_grid
pointer move $item_x $item_y sleep 300 click right sleep 600
sleep 0.5
grim "/out/$impl-3-menu.png" 2>/dev/null
shown && fail "o menu da dock abriu com a grade por cima" || ok "o botão direito esconde a grade (o menu da dock à vista)"
pointer move 640 300 sleep 200 click sleep 300  # fecha o menu: aberto, ele segura a dock à vista
sleep 0.5

# Duas saídas lado a lado: a grade abre na do foco (a segunda), e o clique na dock dela chega à dock. Só na
# versão em Rust: no headless, a dock em GTK 3 numa saída criada depois não recebe o ponteiro, nem sem a
# grade (a dock em si, não a grade: a região que a grade em Python calcula ali é a mesma)
if [ "$impl" = rust ]; then
rm -f /tmp/launched
swaymsg create_output >/dev/null
swaymsg output HEADLESS-1 resolution 1280x720 position 0 0 >/dev/null
swaymsg output HEADLESS-2 resolution 1280x720 position 1280 0 >/dev/null
swaymsg focus output HEADLESS-2 >/dev/null
for _ in $(seq 50); do [ "$(wc -l < "$XDG_RUNTIME_DIR/sway-dock.area" 2>/dev/null)" = 2 ] && break; sleep 0.1; done
sleep 1
[ "$(wc -l < "$XDG_RUNTIME_DIR/sway-dock.area")" = 2 ] && ok "uma área por saída" || fail "a área não tem as duas docks: $(cat "$XDG_RUNTIME_DIR/sway-dock.area")"
open_grid
grim "/out/$impl-4-duas-saidas.png" 2>/dev/null
/bin/swaydesk-vpointer --size 2560x720 move $((item_x + 1280)) $item_y sleep 300 click sleep 500
sleep 1
[ -e /tmp/launched ] && ok "na segunda saída, o clique na dock dela abre o app" || fail "na segunda saída, o clique não chegou à dock"
shown && fail "a grade continuou aberta na segunda saída" || ok "a grade se esconde na segunda saída"
# Na segunda saída, o lugar da dock da primeira não fica livre: um clique ali cai no fundo e fecha a grade
open_grid
/bin/swaydesk-vpointer --size 2560x720 move 1290 200 sleep 300 click sleep 500
sleep 0.8
shown && fail "um clique no fundo da segunda saída não fechou a grade" || ok "o fundo da segunda saída fecha a grade"
swaymsg output HEADLESS-2 unplug >/dev/null 2>&1 || swaymsg output HEADLESS-2 disable >/dev/null
swaymsg focus output HEADLESS-1 >/dev/null
sleep 1.5
fi

# A dock escondida por uma janela (modo padrão): a faixa da borda continua livre, e o ponteiro nela traz a
# dock por cima da grade aberta
cat > /tmp/cover.py <<'PY'
import gi
gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk
GLib.set_prgname("cover-app")
Gtk.init()
window = Gtk.Window(title="Cover")
window.present()
GLib.MainLoop().run()
PY
rm -f /tmp/launched
python3 /tmp/cover.py & cover=$!
for _ in $(seq 50); do swaymsg -t get_tree | grep -q '"app_id": "cover-app"' && break; sleep 0.1; done
pointer move 640 300 sleep 300
sleep 2.5  # a dock desce (atraso e animação)
read -r _ _ _ hh < "$XDG_RUNTIME_DIR/sway-dock.area" 2>/dev/null
[ "${hh:-}" = 2 ] && ok "escondida, a dock publica só a faixa da borda" || fail "escondida, a área é $(cat "$XDG_RUNTIME_DIR/sway-dock.area" 2>/dev/null)"
open_grid
# Com a janela aberta, a dock ganha o item dela: três lugares, e o fixado é o primeiro
pointer move 640 600 sleep 200 move 640 719 sleep 1200 move 584 $item_y sleep 300 click sleep 500
sleep 1
grim "/out/$impl-5-borda.png" 2>/dev/null
shown && fail "a grade continuou aberta depois do clique pela borda" || ok "a grade se esconde depois do clique pela borda"
[ -e /tmp/launched ] && ok "a borda traz a dock por cima da grade, e o clique abre o app" || fail "com a grade aberta, a borda não trouxe a dock"
kill $cover 2>/dev/null

kill "$(cat "$XDG_RUNTIME_DIR/sway-dock.pid")" 2>/dev/null; sleep 1
[ -e "$XDG_RUNTIME_DIR/sway-dock.area" ] && fail "a área ficou depois da dock sair" || ok "a dock tira a área ao sair"
exit "$failed"
