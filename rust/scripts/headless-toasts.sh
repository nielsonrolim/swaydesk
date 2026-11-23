#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa os toasts do sway-notifd num Sway headless: a captura (grim) e os cliques, com o ponteiro virtual
# (swaydesk-vpointer). Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório em /repo, os
# binários em /bin e a pasta das capturas em /out:
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro --group-add $(getent group render | cut -d: -f3) \
#     --device /dev/dri -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-notifd:/bin/swaydesk-notifd:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro -v /tmp/out:/out \
#     -v /usr/share/icons:/usr/share/icons:ro swaydesk-smoke:debian13 /repo/rust/scripts/headless-toasts.sh
#
# Para cada versão (rust e python): três toasts e a captura; o clique no corpo de um toast com a ação
# "default" a invoca e o fecha; o clique no X fecha; o clique num botão de ação. Sai com 1 se algo falhar.
set -u
# Os ids crescem: a, b, c, d (a ordem do sort numérico basta com menos de 10)
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
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

# expect DESCRIÇÃO ESPERADO OBTIDO
expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}

click() { /bin/swaydesk-vpointer --size 1280x720 move "$1" "$2" click sleep 200; sleep 0.6; }

for impl in rust python; do
  SWAY_NOTIFD_IMPL=$impl "$HOME/.local/bin/sway-notifd" 2>/tmp/notifd-$impl.err & daemon=$!
  for _ in $(seq 50); do notify list >/dev/null 2>&1 && break; sleep 0.1; done
  a=$(notify notify "Bateria fraca" "Restam <b>10%</b>. Conecte o carregador." --app Energia --critical)
  b=$(notify notify "Mensagem nova" "Um corpo comprido o bastante para quebrar em mais de uma linha e ser cortado no fim com reticências, porque passa de três linhas de texto no cartão do toast." --app Chat --actions "default,Abrir,reply,Responder,later,Depois")
  c=$(notify notify "Download concluído" "arquivo.iso" --app Navegador)
  sleep 1
  grim "/out/toasts-$impl.png"
  # O mais novo em cima (c), depois b, depois a. O corpo de c não tem "default": só sai da tela
  click 1000 50
  expect "$impl: clique no corpo sem default deixa na lista" "$a $b $c" "$(notify list | tr ' ' '\n' | sort | tr '\n' ' ' | sed 's/ $//')"
  # Agora b em cima: o corpo dele tem "default", que é invocado, e a notificação fecha
  click 1000 60
  expect "$impl: clique no corpo com default fecha" "$a $c" "$(notify list | tr ' ' '\n' | sort | tr '\n' ' ' | sed 's/ $//')"
  # Só a em cima: o X fecha
  click 1240 25
  expect "$impl: o X fecha" "$c" "$(notify list)"
  d=$(notify notify "Pergunta" "Responder agora?" --app Chat --actions "reply,Responder,later,Depois")
  sleep 0.8
  grim "/out/toasts-$impl-actions.png"
  click 1010 97
  expect "$impl: o botão de ação fecha" "$c" "$(notify list)"
  echo "$impl: daemon $(readlink /proc/$daemon/exe 2>/dev/null || echo python)"
  kill "$daemon"; wait "$daemon" 2>/dev/null
  sleep 0.5
done
exit "$failed"
