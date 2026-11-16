#!/bin/bash
# Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório em /repo:
#   docker run --rm -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro -v $PWD:/repo:ro \
#     swaydesk-smoke:debian13 timeout 60 /repo/rust/scripts/headless-keys-dotfiles.sh
# Espera: o 00-keys.conf com a pasta keys do repositório, o base.conf e o dwm.conf carregados, e um reload só
# (o config principal carregado 3 vezes: o início e as duas passadas do reload).
# Os dotfiles depois de um git pull sem o install.sh: o config é um link para o repositório, sem ~/.config/sway/keys
# nem 00-keys.conf, e só os links que existiam antes dos perfis. A sessão tem de ganhar os atalhos sozinha (o
# exec_always do config, com o sway-desktop-config keys)
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 LANG=C.UTF-8
mkdir -p $HOME/.config/sway/config.d $HOME/.local/bin $HOME/.local/share/wallpaper $HOME/.config/sway-desktop && mkdir -m 700 $XDG_RUNTIME_DIR
: > $HOME/.local/share/wallpaper/current.jpg
ln -s /repo/home/.config/sway/config $HOME/.config/sway/config
for f in sway-desktop-config desktop_config.py sway_i18n.py sway_rust.py; do [ -e /repo/home/.local/bin/$f ] && ln -s /repo/home/.local/bin/$f $HOME/.local/bin/; done
printf 'shortcuts = "dwm"\n' > $HOME/.config/sway-desktop/config.toml
eval "$(dbus-daemon --session --fork --print-address=1 | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"
sway -d -c $HOME/.config/sway/config > /tmp/sway.log 2>&1 &
sleep 6
echo "--- 00-keys.conf"; cat $HOME/.config/sway/config.d/00-keys.conf
echo "--- loaded"; grep -E "Loading config from .*keys/" /tmp/sway.log | sed 's/.*Loading/Loading/' | sort | uniq -c
echo "--- main config loads"; grep -c "Loading config from $HOME/.config/sway/config$" /tmp/sway.log
