#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o popup de rede em Rust num Sway headless, com um NetworkManager falso (fake-nm.py) num barramento
# próprio no lugar do do sistema. Roda dentro do contêiner swaydesk-smoke:debian13 com os binários
# swaydesk-network-popup e swaydesk-vpointer em /bin e uma pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro -v $PWD:/repo:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-network-popup:/bin/swaydesk-network-popup:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-network-popup.sh
#
# Só o cabo (um PC sem placa de Wi-Fi): o interruptor do Wi-Fi está à vista e desativado, e o clique nele não chama o
# nmcli. A placa de Wi-Fi espetada com o popup aberto (SIGUSR1 no falso): o interruptor fica ativo e o
# clique chama o "nmcli radio wifi off". A versão em Python precisa da libnm (gir1.2-nm-1.0), que a imagem
# não tem: ela fica para a rodada na VM. Sai com 1 se algo falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" /tmp/fake && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do [ -f "$f" ] && ln -sf "$f" "$HOME/.local/bin/"; done
cp /bin/swaydesk-network-popup "$HOME/.local/bin/"
cat > /tmp/fake/nmcli <<'FAKE'
#!/bin/sh
printf '%s\n' "$*" >>/tmp/fake/nmcli.calls
FAKE
chmod +x /tmp/fake/nmcli
export PATH=/tmp/fake:$PATH
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
failed=0
expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
click() { /bin/swaydesk-vpointer --size 1280x720 move "$1" "$2" click sleep 200 move 600 600 sleep 100; sleep 0.8; }
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
bus=$(dbus-daemon --session --fork --print-address) || exit 1
export DBUS_SYSTEM_BUS_ADDRESS=$bus
DBUS_SYSTEM_BUS_ADDRESS=$bus python3 /repo/rust/scripts/fake-nm.py > /tmp/nm.ready 2>/tmp/nm.err & fake=$!
trap 'kill $keeper $fake 2>/dev/null' EXIT
for _ in $(seq 50); do grep -q ready /tmp/nm.ready 2>/dev/null && break; sleep 0.1; done
sleep 0.5

# O interruptor do Wi-Fi, no canto de cima do popup (o "Wi-Fi" à esquerda, o "Network" à direita)
SW_X=${SW_X:-987} SW_Y=${SW_Y:-28}
SWAY_NETWORK_POPUP_IMPL=rust "$HOME/.local/bin/waybar-network-popup" 2>>/tmp/popup.err & popup=$!
sleep 3
echo "popup $(readlink /proc/$popup/exe)"
grim /out/network-no-wifi.png
click "$SW_X" "$SW_Y"
expect "sem placa: o clique no Wi-Fi não chama o nmcli" "" "$(cat /tmp/fake/nmcli.calls 2>/dev/null)"
expect "sem placa: o popup continua aberto" "aberto" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"

rm -f /tmp/fake/nmcli.calls
kill -USR1 "$fake"
sleep 1.5
grim /out/network-wifi.png
click "$SW_X" "$SW_Y"
expect "placa espetada: o clique desliga o Wi-Fi" "radio wifi off" "$(cat /tmp/fake/nmcli.calls 2>/dev/null)"
expect "placa espetada: o popup continua aberto" "aberto" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"
kill "$popup" 2>/dev/null; wait "$popup" 2>/dev/null
grep -v '^$' /tmp/popup.err /tmp/nm.err | head -20
exit "$failed"
