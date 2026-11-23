#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o popup de áudio num Sway headless, com pw-dump, wpctl e pactl falsos (o contêiner não tem
# PipeWire): a captura (grim) e os cliques, com o ponteiro virtual (swaydesk-vpointer). Roda dentro do
# contêiner swaydesk-smoke:debian13, como o headless-toasts.sh, com swaydesk-audio-popup e
# swaydesk-vpointer em /bin.
#
# Para cada versão (rust e, com o python3-cairo, python): a placa com duas portas e um microfone; o clique na outra porta (o
# pactl set-sink-port e o wpctl set-default), no mudo, e um clique na parte transparente da superfície,
# abaixo do cartão, que tem de passar para o clique fora e fechar o popup. Sai com 1 se algo falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" /tmp/fakebin && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do [ -f "$f" ] && ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
# Com /tmp/lineout, a placa ganha uma terceira porta (a lista cresce com o popup aberto)
cat > /tmp/fakebin/pw-dump <<'SCRIPT'
#!/bin/sh
extra=
[ -e /tmp/lineout ] && extra=',{"index": 2, "direction": "Output", "device": 1, "name": "analog-output-lineout", "description": "Line Out", "profiles": [1], "devices": [1], "available": "yes"}'
cat <<JSON
[{"id": 40, "type": "PipeWire:Interface:Device",
  "info": {"props": {"device.description": "Built-in Audio"},
           "params": {"EnumRoute": [{"index": 0, "direction": "Output", "device": 1, "name": "analog-output-speaker", "description": "Speakers", "profiles": [1], "devices": [1], "available": "yes"},
                                    {"index": 1, "direction": "Output", "device": 1, "name": "analog-output-headphones", "description": "Headphones", "profiles": [1], "devices": [1], "available": "yes"}$extra],
                      "Route": [{"index": 0, "direction": "Output", "device": 1, "props": {"channelVolumes": [0.343, 0.343], "mute": false}}],
                      "Profile": [{"index": 1}]}}},
 {"id": 50, "type": "PipeWire:Interface:Node",
  "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.pci", "device.id": 40, "card.profile.device": 1, "node.description": "Built-in Audio Analog Stereo"},
           "params": {"Props": [{"channelVolumes": [0.343, 0.343], "mute": false}]}}},
 {"id": 51, "type": "PipeWire:Interface:Node",
  "info": {"props": {"media.class": "Audio/Source", "node.name": "alsa_input.usb", "node.description": "USB Microfone"},
           "params": {"Props": [{"channelVolumes": [0.512], "mute": true}]}}},
 {"id": 60, "type": "PipeWire:Interface:Metadata",
  "metadata": [{"key": "default.audio.sink", "value": {"name": "alsa_output.pci"}}, {"key": "default.audio.source", "value": {"name": "alsa_input.usb"}}]}]
JSON
SCRIPT
printf '#!/bin/sh\necho "wpctl $*" >>/tmp/calls\n' > /tmp/fakebin/wpctl
# O subscribe repassa o que o teste escreve em /tmp/events
printf '#!/bin/sh\n[ "$1" = subscribe ] && exec tail -n0 -F /tmp/events 2>/dev/null\necho "pactl $*" >>/tmp/calls\n' > /tmp/fakebin/pactl
printf '#!/bin/sh\nexit 0\n' > /tmp/fakebin/pavucontrol
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
click() { /bin/swaydesk-vpointer --size 1280x720 move "$1" "$2" click sleep 200; sleep 0.6; }
impls="rust python"
# O waybar-audio em Python precisa do python3-cairo, que o contêiner pode não ter
python3 -c 'import cairo' 2>/dev/null || { impls=rust; echo "python: pulado (sem o python3-cairo)"; }
for impl in $impls; do
  : >/tmp/calls; rm -f /tmp/lineout; : >/tmp/events
  SWAY_AUDIO_IMPL=$impl "$HOME/.local/bin/waybar-audio" 2>>/tmp/popup.err & popup=$!
  sleep 2
  echo "$impl: popup $(readlink /proc/$popup/exe)"
  grim "/out/audio-$impl.png"
  click 1100 142
  expect "$impl: a outra porta" "pactl set-sink-port alsa_output.pci analog-output-headphones|wpctl set-default 50" "$(paste -sd'|' /tmp/calls)"
  : >/tmp/calls
  click 965 50
  expect "$impl: o mudo" "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle" "$(cat /tmp/calls)"
  expect "$impl: aberto depois dos cliques" "aberto" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"
  # A lista cresce (um aviso do pactl subscribe) e o item novo, embaixo, recebe o clique
  touch /tmp/lineout; echo "Event 'change' on card #40" >>/tmp/events; sleep 1.5
  grim "/out/audio-$impl-grown.png"
  : >/tmp/calls
  click 1100 190
  expect "$impl: o item novo, embaixo, recebe o clique" "pactl set-sink-port alsa_output.pci analog-output-lineout|wpctl set-default 50" "$(paste -sd'|' /tmp/calls)"
  # O link desceu para fora da área antiga do cartão: só chega a ele se a região de entrada cresceu. O cartão do
  # GTK 3 (python) é 12 px mais alto que o do GTK 4 (rust): o link fica em 351, não em 339
  link_y=339
  [ "$impl" = python ] && link_y=351
  : >"$XDG_RUNTIME_DIR/waybar-popups.log"  # sem a linha da rodada anterior: um clique perdido dá ""
  click 1214 "$link_y"
  expect "$impl: a região de entrada acompanha o cartão" "pavucontrol" "$(tail -n 1 "$XDG_RUNTIME_DIR/waybar-popups.log" | grep -o 'pavucontrol\|clique fora')"
  # O popup fecha com o clique; se não fechou (uma falha acima), não fica esperando para sempre
  for _ in $(seq 50); do kill -0 "$popup" 2>/dev/null || break; sleep 0.1; done
  kill "$popup" 2>/dev/null; wait $popup 2>/dev/null
  SWAY_AUDIO_IMPL=$impl "$HOME/.local/bin/waybar-audio" 2>>/tmp/popup.err & popup=$!
  sleep 2
  click 1100 650  # abaixo do cartão, ainda dentro da superfície de 900 px
  expect "$impl: o clique na parte transparente fecha" "fechado" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"
  kill $popup 2>/dev/null; wait $popup 2>/dev/null; sleep 0.5
done
grep -v '^$' /tmp/popup.err | grep -v 'session bus' | head -20
exit "$failed"
