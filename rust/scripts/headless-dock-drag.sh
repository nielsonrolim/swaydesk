#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o arrasto que reordena os fixados da dock (Rust e Python) num Sway headless, com o ponteiro virtual
# (swaydesk-vpointer). Roda dentro do contêiner swaydesk-smoke:debian13, como o headless-dock.sh:
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) --group-add $(getent group render | cut -d: -f3) --device /dev/dri \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-dock:/bin/swaydesk-dock:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     -v /usr/share/icons:/usr/share/icons:ro -v /tmp/out:/out swaydesk-smoke:debian13 \
#     /repo/rust/scripts/headless-dock-drag.sh [rust|python]
#
# Quatro apps fixados (a, b, c, d) e o botão da grade, lugares de 56 px com a dock no centro de uma tela de
# 1280 px: os centros dos fixados ficam em x = 528, 584, 640 e 696. O item arrastado passa por um vizinho
# quando o ponteiro cruza o centro do vizinho, para a direita e para a esquerda, e fica no lugar sob o
# ponteiro (não um adiante). Sai com 1 se algum caso falhar.
#
# Duas rodadas: com a animação do arrasto (o padrão) e com gtk-enable-animations=false no settings.ini do
# GTK 3 e do GTK 4. A ordem final é a mesma nas duas; no meio do arrasto, o item segue o ponteiro (shifted
# no dump do kill -USR2) só com a animação, e depois de solto nada fica fora do lugar nem animando
# (shifted=0 drag_animating=false: o timer parou).
set -u
. /repo/rust/scripts/lib-wait.sh
impl=${1:-rust}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" "$HOME/.local/share/applications" \
  "$HOME/.local/share/sway-dock" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
for app in a b c d; do
  cat > "$HOME/.local/share/applications/drag-$app.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Drag $app
Exec=true
Icon=utilities-terminal
DESKTOP
done
pins=$HOME/.local/share/sway-dock/pinned
printf 'drag-a\ndrag-b\ndrag-c\ndrag-d\n' > "$pins"
mkdir -p "$HOME/.config/sway-dock"
printf 'launcher = "true"\n' > "$HOME/.config/sway-dock/config.toml"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
mode=on
pointer() { /bin/swaydesk-vpointer --size 1280x720 "$@"; }
shot() { grim "/out/$impl-drag-$mode-$1.png" 2>/dev/null; }
# O estado da dock no log (kill -USR2): o valor de um campo do último dump
dump() {
  local before; before=$(grep -c "dock 0:" /tmp/dock.log)
  kill -USR2 "$pid"
  for _ in $(seq $((ticks * 2))); do [ "$(grep -c "dock 0:" /tmp/dock.log)" -gt "$before" ] && break; sleep 0.05; done
  grep "dock 0:" /tmp/dock.log | tail -1
}
field() { sed -n "s/.* $1=\([^ ]*\).*/\1/p"; }
order() { sed 's/^drag-//' "$pins" | paste -sd ' '; }
y=688
# Segura o botão em FROM, anda até TO em passos de 4 px (e até a altura TO_Y, se dada) e solta. Confere
# o desenho no meio do arrasto (menos nas pontas, onde o item para na borda da fileira, às vezes no próprio
# lugar) e depois de solto
drag() {
  local from=$1 to=$2 name=$3 to_y=${4:-$y} step=4 args=() x state
  local follows=yes; [[ $name == *ponta* ]] && follows=no
  [ "$to" -lt "$from" ] && step=-4
  args+=(move "$from" $y sleep 200 press sleep 100)
  for ((x = from + step; (step > 0 && x < to) || (step < 0 && x > to); x += step)); do
    args+=(move "$x" $y sleep 15)
  done
  args+=(move "$to" "$to_y" sleep 300)
  pointer "${args[@]}"
  shot "$name"
  state=$(dump)
  if [ "$mode" = on ] && [ $follows = yes ] && [ "$(field shifted <<< "$state")" = 0 ]; then
    fail "$name: no meio do arrasto, o item não seguiu o ponteiro ($state)"
  elif [ "$mode" = off ] && [ "$(field shifted <<< "$state")" != 0 ]; then
    fail "$name: sem animação, algo saiu do lugar ($state)"
  fi
  pointer release sleep 700
  state=$(dump)
  if [ "$(field shifted <<< "$state")" != 0 ] || [ "$(field drag_animating <<< "$state")" != false ]; then
    fail "$name: depois de solto, ficou fora do lugar ou animando ($state)"
  fi
}
check() {  # NOME ORDEM-ESPERADA
  local got; got=$(order)
  [ "$got" = "$2" ] && ok "$1 ($got)" || fail "$1: ficou \"$got\", esperado \"$2\""
}

start_dock() {
  : > /tmp/dock.log
  env G_DEBUG=fatal-criticals SWAY_DOCK_IMPL=$impl "$HOME/.local/bin/sway-dock" >> /tmp/dock.log 2>&1 &
  for _ in $(seq $ticks); do [ -e "$XDG_RUNTIME_DIR/sway-dock.pid" ] && break; sleep 0.1; done
  sleep 1.5
  pid=$(cat "$XDG_RUNTIME_DIR/sway-dock.pid" 2>/dev/null)
  if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
    fail "a dock não subiu ($mode)"; cat /tmp/dock.log; exit 1
  fi
}
stop_dock() {
  kill "$pid"
  for _ in $(seq $ticks); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
  rm -f "$XDG_RUNTIME_DIR/sway-dock.pid"
  grep -v "^$" /tmp/dock.log | grep -iv "old docks\|dock 0:" | head -20 | sed "s/^/log ($mode): /"
}

round() {
  printf 'drag-a\ndrag-b\ndrag-c\ndrag-d\n' > "$pins"
  start_dock
  shot 0-inicio
  # Para a direita: o a só passa o b depois do centro do b (584)
  drag 528 578 1-antes-do-centro
  check "para a direita, antes do centro do vizinho, não troca" "a b c d"
  drag 528 590 2-depois-do-centro
  check "para a direita, depois do centro do vizinho, troca com ele" "b a c d"
  # O b (agora no lugar 0) sobre o lugar 2, passado o centro do c (640) e antes do centro do d (696): fica no
  # lugar 2, não no 3
  drag 528 650 3-dois-lugares
  check "para a direita, fica no lugar sob o ponteiro" "a c b d"
  # Para a esquerda: o d (lugar 3) passa o b (640) e o c (584), não o a (528)
  drag 696 578 4-esquerda
  check "para a esquerda, fica no lugar sob o ponteiro" "a d c b"
  # As pontas: além do primeiro e do último
  drag 640 495 5-ponta-esquerda
  check "solto antes do primeiro, vai para o começo" "c a d b"
  drag 528 770 6-ponta-direita
  check "solto depois do último fixado (sobre a grade), vai para o fim" "a d b c"
  # Solto fora da dock, acima dela: assenta no lugar sob o x do ponteiro, sem nada fora do lugar
  drag 528 590 7-solto-fora 400
  check "solto acima da dock, fica no lugar sob o ponteiro" "d a b c"
  # Os apps instalados mudam no meio do arrasto: ao soltar, a dock recria os itens, e o resto do assentar
  # não desenha o item antigo, que já não está na fileira (com G_DEBUG=fatal-criticals, um critical
  # derrubaria a dock)
  pointer move 528 $y sleep 200 press sleep 100 move 540 $y sleep 15 move 552 $y sleep 15 move 560 $y sleep 200
  printf '[Desktop Entry]\nType=Application\nName=Drag e\nExec=true\n' > "$HOME/.local/share/applications/drag-e-$mode.desktop"
  sleep 1.5
  pointer release sleep 700
  if kill -0 "$pid" 2>/dev/null; then
    state=$(dump)
    [ "$(field shifted <<< "$state")" = 0 ] && [ "$(field drag_animating <<< "$state")" = false ] \
      && ok "os apps mudam no meio do arrasto, e a dock segue no ar" \
      || fail "os apps mudaram no arrasto: ficou algo fora do lugar ou animando ($state)"
  else
    fail "os apps mudaram no meio do arrasto, e a dock caiu"; tail -5 /tmp/dock.log
  fi
  stop_dock
}

mode=on
round
mode=off
for gtk in gtk-3.0 gtk-4.0; do
  mkdir -p "$HOME/.config/$gtk"
  printf '[Settings]\ngtk-enable-animations=false\n' > "$HOME/.config/$gtk/settings.ini"
done
round

exit $failed
