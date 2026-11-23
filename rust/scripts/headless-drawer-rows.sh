#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# A grade de apps (Rust ou Python) numa tela baixa, com mais apps do que cabem: a última linha à vista sai
# inteira, com os nomes, e não cortada pelo rodapé. Roda dentro do contêiner swaydesk-smoke:debian13, com o
# repositório em /repo, o binário da grade em /bin e uma pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) --group-add $(getent group render | cut -d: -f3) --device /dev/dri \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-drawer:/bin/swaydesk-drawer:ro \
#     -v /usr/share/icons:/usr/share/icons:ro -v /tmp/out:/out swaydesk-smoke:debian13 \
#     /repo/rust/scripts/headless-drawer-rows.sh [rust|python] [LARGURAxALTURA] [APPS]
#
# Os apps são falsos, com nomes de duas linhas, como os traduzidos ("Gerenciador de arquivos Thunar"), além dos
# poucos da imagem. A conferência é pela captura: /out/<impl>-<tela>-<apps>.png.
set -u
impl=${1:-rust} size=${2:-1280x800} count=${3:-20}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" "$HOME/.local/share/applications" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do cp "$f" "$HOME/.local/bin/"; done
icons=(utilities-terminal system-file-manager accessories-text-editor preferences-system audio-volume-high
       camera-photo applications-graphics internet-web-browser media-removable bluetooth)
for i in $(seq -w 1 "$count"); do
  cat > "$HOME/.local/share/applications/app$i.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Gerenciador de arquivos número $i
Exec=true
Icon=${icons[$((10#$i % ${#icons[@]}))]}
DESKTOP
done
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution %s\n' "$size" > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
sleep 0.5
SWAY_DRAWER_IMPL=$impl "$HOME/.local/bin/sway-drawer" start > /tmp/drawer.log 2>&1 &
sleep 2
SWAY_DRAWER_IMPL=$impl "$HOME/.local/bin/sway-drawer"
for _ in $(seq 30); do [ -e "$XDG_RUNTIME_DIR/sway-drawer.shown" ] && break; sleep 0.1; done
sleep 1.5
grim "/out/$impl-$size-$count.png"
[ -e "$XDG_RUNTIME_DIR/sway-drawer.shown" ] && echo "ok      $impl $size $count apps: /out/$impl-$size-$count.png" \
  || { echo "FALHOU  a grade não abriu"; cat /tmp/drawer.log; exit 1; }
