#!/bin/bash
# Testa o popup "Sistema" num Sway headless: a captura (grim) e o "Encerrar" em dois cliques, com o
# ponteiro virtual (swaydesk-vpointer). Roda dentro do contêiner swaydesk-smoke:debian13, como o
# headless-toasts.sh, com os binários swaydesk-system-popup e swaydesk-vpointer em /bin.
#
# Para cada versão (rust e python): um sh em laço (o primeiro da lista por CPU), a captura, um clique no
# botão dele (vira "Confirmar"), a captura, o segundo clique (SIGTERM), e o laço tem de ter saído. Depois, os
# perfis de energia com um busctl e um upower falsos: a seção aparece e o clique troca o perfil. Sai com 1
# se algo falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/sway-desktop" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do [ -f "$f" ] && ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
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
click() { /bin/swaydesk-vpointer --size 1280x720 move "$1" "$2" click sleep 200 move 600 600 sleep 100; sleep 0.6; }
for impl in rust python; do
  sh -c 'while :; do :; done' & busy=$!
  sleep 1
  SWAY_SYSTEM_POPUP_IMPL=$impl "$HOME/.local/bin/waybar-system-popup" 2>>/tmp/popup.err & popup=$!
  sleep 3.5
  echo "$impl: popup $(readlink /proc/$popup/exe)"
  grim "/out/system-$impl.png"
  click 1213 267
  grim "/out/system-$impl-armed.png"
  click 1213 267
  sleep 0.5
  expect "$impl: dois cliques encerram o laço" "saiu" "$(kill -0 $busy 2>/dev/null && echo vivo || echo saiu)"
  expect "$impl: o popup continua aberto" "aberto" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"
  grim "/out/system-$impl-after.png"
  kill $busy 2>/dev/null; wait $busy 2>/dev/null
  kill $popup 2>/dev/null; wait $popup 2>/dev/null; sleep 0.5
done

# Os perfis de energia: um busctl e um upower falsos no PATH, sem a bateria da máquina (só um mouse). No
# "auto", a seção aparece com os dois perfis que o daemon oferece, e o clique no primeiro grava o set-property
mkdir -p /tmp/fake
cat > /tmp/fake/busctl <<'FAKE'
#!/bin/sh
case "$*" in
  *set-property*) printf '%s\n' "$*" >>/tmp/fake/set ;;
  *' Profiles') echo '{"type":"aa{sv}","data":[{"Profile":{"type":"s","data":"power-saver"}},{"Profile":{"type":"s","data":"balanced"}}]}' ;;
  *' ActiveProfile') echo 's "balanced"' ;;
  *) exit 1 ;;
esac
FAKE
cat > /tmp/fake/upower <<'FAKE'
#!/bin/sh
printf '%s\n' /org/freedesktop/UPower/devices/line_power_AC /org/freedesktop/UPower/devices/mouse_hidpp_battery_0
FAKE
chmod +x /tmp/fake/busctl /tmp/fake/upower
for impl in rust python; do
  rm -f /tmp/fake/set
  PATH=/tmp/fake:$PATH SWAY_SYSTEM_POPUP_IMPL=$impl "$HOME/.local/bin/waybar-system-popup" 2>>/tmp/popup.err & popup=$!
  sleep 3.5
  grim "/out/system-$impl-profiles.png"
  click 995 261  # o "Power saver"
  sleep 0.5
  expect "$impl: o clique troca o perfil" "power-saver" "$(sed -n 's/.* s //p' /tmp/fake/set 2>/dev/null)"
  expect "$impl: o popup continua aberto com os perfis" "aberto" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"
  kill $popup 2>/dev/null; wait $popup 2>/dev/null; sleep 0.5
done
grep -v '^$' /tmp/popup.err | grep -v 'session bus' | head -20
exit "$failed"
