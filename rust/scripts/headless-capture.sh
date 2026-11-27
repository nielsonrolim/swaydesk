#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa os painéis de captura e de gravação (sway-capture) num Sway headless. Roda dentro do contêiner
# swaydesk-smoke:debian13, como o headless-toasts.sh, com o swaydesk-capture e o swaydesk-vpointer em /bin.
#
# O sway-shot e o sway-rec são falsos: anotam a chamada. Para cada versão (rust e python):
# - o painel de captura abre (captura), e um clique em "Tela inteira" chama o sway-shot com "full" depois de
#   fechar;
# - o painel de gravação, com o áudio lembrado em ~/.cache/sway-capture.json ("mic"), chama o sway-rec com
#   "region mic";
# - um clique fora fecha sem chamar nada.
# Sai com 1 se algo falhar.
set -u
. /repo/rust/scripts/lib-wait.sh
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.cache" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
cp /repo/home/.local/bin/* "$HOME/.local/bin/"
cp /bin/swaydesk-capture "$HOME/.local/bin/"
for name in sway-shot sway-rec; do
  printf '#!/bin/sh\necho "%s $*" >>/tmp/calls\n' "$name" > "$HOME/.local/bin/$name"
  chmod +x "$HOME/.local/bin/$name"
done
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
failed=0
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5
expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
open_panel() {  # impl tipo
  SWAY_CAPTURE_IMPL=$1 "$HOME/.local/bin/sway-capture" "$2" 2>>/tmp/capture.err & panel=$!
  sleep 3
}
closed() {
  for _ in $(seq $ticks); do kill -0 "$panel" 2>/dev/null || { echo fechado; return; }; sleep 0.1; done
  echo aberto
}
# O botão de um alvo: os três ficam lado a lado no centro do cartão
target_x() { case $1 in region) echo 520 ;; full) echo 640 ;; window) echo 760 ;; esac; }
for impl in ${IMPLS:-rust python}; do
  : >/tmp/calls
  printf '{"audio": "mic", "gradia": false}' > "$HOME/.cache/sway-capture.json"
  open_panel "$impl" shot
  echo "$impl: $(readlink /proc/$panel/exe)"
  grim "/out/capture-$impl-shot.png"
  /bin/swaydesk-vpointer --size 1280x720 move "$(target_x full)" 300 click sleep 200
  expect "$impl: o clique fecha o painel" "fechado" "$(closed)"
  sleep 0.6
  expect "$impl: chama o sway-shot depois de fechar" "sway-shot full" "$(cat /tmp/calls)"
  : >/tmp/calls
  open_panel "$impl" rec
  grim "/out/capture-$impl-rec.png"
  /bin/swaydesk-vpointer --size 1280x720 move "$(target_x region)" 300 click sleep 200
  closed >/dev/null
  sleep 0.6
  expect "$impl: o áudio lembrado vai para o sway-rec" "sway-rec region mic" "$(cat /tmp/calls)"
  : >/tmp/calls
  open_panel "$impl" shot
  /bin/swaydesk-vpointer --size 1280x720 move 60 60 click sleep 200
  expect "$impl: clicar fora fecha" "fechado" "$(closed)"
  sleep 0.6
  expect "$impl: sem captura" "" "$(cat /tmp/calls)"
done
grep -v '^$' /tmp/capture.err | grep -v 'session bus\|a11y\|AT-SPI' | head -10
exit "$failed"
