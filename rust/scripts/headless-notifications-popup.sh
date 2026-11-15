#!/bin/bash
# Testa o popup da central de notificações num Sway headless: a captura (grim) e os cliques, com o
# ponteiro virtual (swaydesk-vpointer). Roda dentro do contêiner swaydesk-smoke:debian13, como o
# headless-toasts.sh (o mesmo docker run, com os binários swaydesk-notifd, swaydesk-notifications-popup e
# swaydesk-vpointer em /bin).
#
# Para cada versão do popup (rust e python), com o daemon em Rust: três notificações de dois apps, a
# captura, e os cliques no "Limpar" de um grupo, no X de uma linha e no "Sempre" do não perturbe,
# conferidos pelo List do daemon, e o popup segue aberto. Depois, uma crítica com a central aberta: ela fica
# à esquerda da central, e o X dela a dispensa sem fechar a central. Sai com 1 se algo falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/sway-desktop" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
DBUS_SESSION_BUS_ADDRESS=$(dbus-daemon --session --fork --print-address=1)
export DBUS_SESSION_BUS_ADDRESS
notify() { python3 /repo/rust/scripts/notify.py "$@"; }
failed=0
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5
SWAY_NOTIFD_IMPL=rust "$HOME/.local/bin/sway-notifd" 2>/tmp/notifd.err &
for _ in $(seq 50); do notify list >/dev/null 2>&1 && break; sleep 0.1; done

expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
click() { /bin/swaydesk-vpointer --size 1280x720 move "$1" "$2" click sleep 200; sleep 0.8; }
open_popup() {
  SWAY_NOTIFICATIONS_POPUP_IMPL=$1 "$HOME/.local/bin/waybar-notifications-popup" 2>>/tmp/popup.err & popup=$!
  for _ in $(seq 50); do [ -e "$XDG_RUNTIME_DIR/waybar-notifications-popup.pid" ] && break; sleep 0.1; done
  sleep 1.2
}
close_popup() { kill "$popup" 2>/dev/null; wait "$popup" 2>/dev/null; sleep 0.5; }

for impl in rust python; do
  c1=$(notify notify "Mensagem nova" "Oi, tudo bem?" --app Chat --actions "default,Abrir,reply,Responder")
  n1=$(notify notify "Download concluído" "arquivo.iso" --app Navegador)
  c2=$(notify notify "Outra mensagem" "<b>Importante</b>: reunião às 15h" --app Chat)
  open_popup "$impl"
  grim "/out/popup-$impl.png"
  echo "$impl: popup $(readlink /proc/$popup/exe)"
  click 1237 54  # o "Limpar" do grupo Chat
  expect "$impl: Limpar do grupo tira só o grupo" "$n1" "$(notify list)"
  click 1215 520  # o "Sempre" do não perturbe
  expect "$impl: o não perturbe muda pelo popup" "on" "$(notify dnd)"
  grim "/out/popup-$impl-after.png"
  click 1232 89  # o X da linha que sobrou (só aparece sob o ponteiro, mas o clique vale)
  expect "$impl: o X da linha dispensa" "" "$(notify list)"
  expect "$impl: o popup continua aberto depois dos cliques" "aberto" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"
  close_popup
  notify clear; notify set-dnd off
  # Uma crítica fica na tela com a central aberta, à esquerda dela (a central ocupa o canto dos toasts).
  # O X dela fica a ~32 px da borda direita do toast: 1280 - 6 - (380 + 8) - 32
  crit=$(notify notify "Bateria fraca" "Restam 5%" --app Energia --critical)
  open_popup "$impl"
  grim "/out/popup-$impl-critical.png"
  click 854 26
  expect "$impl: o X da crítica ao lado da central a dispensa" "" "$(notify list)"
  expect "$impl: a central continua aberta depois do clique no toast" "aberto" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"
  close_popup
  notify clear
done
exit "$failed"
