#!/bin/bash
# Testa o [enabled] do config.toml numa sessão Sway headless: o sway-if-enabled nos exec, a grade que confere
# sozinha o app_grid e o sway-waybar com o config filtrado. Roda dentro do contêiner swaydesk-smoke:debian13,
# com o repositório em /repo e os binários swaydesk-dock, swaydesk-drawer e swaydesk-desktop-config em /bins:
#
#   docker run --rm -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro -v $PWD:/repo:ro \
#     -v $PWD/rust/target/debian13/release:/bins:ro swaydesk-smoke:debian13 timeout 240 /repo/rust/scripts/headless-enabled.sh
#
# A imagem não tem a Waybar: uma falsa registra o -c que o sway-waybar passa. Os casos: com dock e app_grid
# desligados, nem a dock nem a grade sobem, e o Super (sway-drawer) não abre nada; a barra sobe com o config
# sem o módulo do tempo; com um modelo quebrado, sobe com o modelo e avisa; ligando de novo e repetindo os
# exec, a dock e a grade sobem. Sai com 1 se algum caso falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/sway-desktop" "$HOME/.config/waybar" /tmp/fake && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for b in dock drawer desktop-config; do cp "/bins/swaydesk-$b" "$HOME/.local/bin/"; done
cp /repo/home/.config/waybar/config.jsonc "$HOME/.config/waybar/config.jsonc"
cat > /tmp/fake/waybar <<'EOF'
#!/bin/sh
echo "$*" >> /tmp/waybar-args
sleep 3600
EOF
chmod +x /tmp/fake/waybar
export PATH=/tmp/fake:$PATH

config() {  # dock app_grid weather
  printf '[enabled]\ndock = %s\napp_grid = %s\n[enabled.bar]\nweather = %s\n' "$1" "$2" "$3" > "$HOME/.config/sway-desktop/config.toml"
}
config false false false

cat > /tmp/sway.conf <<'EOF'
output HEADLESS-1 resolution 1280x720
set $bin ~/.local/bin
exec $bin/sway-if-enabled dock sh -c '$bin/sway-dock'
exec $bin/sway-if-enabled app_grid $bin/sway-drawer start
EOF
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8 GTK_A11Y=none
eval "$(dbus-daemon --session --fork --print-address=1 | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
running() { pgrep -x "$1" >/dev/null; }
wait_running() {  # nome, 1 ou 0
  for _ in $(seq 60); do
    if running "$1"; then [ "$2" = 1 ] && return 0; else [ "$2" = 0 ] && return 0; fi
    sleep 0.1
  done
  return 1
}

sleep 4
running sway-dock && fail "a dock subiu desligada" || ok "dock = false: a dock não sobe"
running sway-drawer && fail "a grade subiu desligada" || ok "app_grid = false: a grade não sobe"
grep -q "dock is off in \[enabled\]" /tmp/sway.log && ok "o log diz que a dock está desligada" || fail "sem o aviso no log"
timeout 10 "$HOME/.local/bin/sway-drawer"; rc=$?
sleep 1
[ $rc = 0 ] && ! running sway-drawer && ok "o Super (sway-drawer) não abre a grade desligada" || fail "o sway-drawer abriu ou falhou ($rc)"

# A barra: o config sem o módulo do tempo, a partir do modelo dos dotfiles
"$HOME/.local/bin/sway-waybar" > /tmp/waybar.log 2>&1 & loop=$!
for _ in $(seq 50); do [ -s /tmp/waybar-args ] && break; sleep 0.1; done
gen=$HOME/.config/swaydesk-desktop/waybar/config.jsonc
if grep -qx -- "-c $gen" /tmp/waybar-args; then ok "a Waybar sobe com o config filtrado"; else fail "a Waybar subiu com: $(cat /tmp/waybar-args)"; fi
python3 - "$gen" <<'PY' && ok "o filtrado não tem o custom/weather e tem o resto" || fail "o filtrado está errado"
import json, re, sys
text = re.sub(r"^//.*$", "", open(sys.argv[1]).read(), flags=re.M)
data = json.loads(text)
mods = data["modules-left"] + data["modules-center"] + data["modules-right"]
assert "custom/weather" not in mods and "custom/clock" in mods and "tray" in mods, mods
PY
pkill -f "sleep 3600"; kill $loop 2>/dev/null; wait $loop 2>/dev/null
: > /tmp/waybar-args

# Um modelo quebrado: a Waybar sobe com ele mesmo (a barra nunca some), e o log avisa
echo '{ quebrado' > "$HOME/.config/waybar/config.jsonc"
"$HOME/.local/bin/sway-waybar" > /tmp/waybar2.log 2>&1 & loop=$!
for _ in $(seq 50); do [ -s /tmp/waybar-args ] && break; sleep 0.1; done
grep -qx -- "-c $HOME/.config/waybar/config.jsonc" /tmp/waybar-args && ok "modelo quebrado: sobe com o modelo" || fail "modelo quebrado: $(cat /tmp/waybar-args)"
grep -q "was not generated" /tmp/waybar2.log && ok "e avisa no log" || fail "sem o aviso: $(cat /tmp/waybar2.log)"
pkill -f "sleep 3600"; kill $loop 2>/dev/null; wait $loop 2>/dev/null

# Ligando de novo: os mesmos exec sobem a dock e a grade
config true true true
swaymsg -q 'exec ~/.local/bin/sway-if-enabled dock ~/.local/bin/sway-dock'
swaymsg -q 'exec ~/.local/bin/sway-if-enabled app_grid ~/.local/bin/sway-drawer start'
wait_running sway-dock 1 && ok "dock = true: a dock sobe" || { fail "a dock não subiu"; tail -20 /tmp/sway.log; }
wait_running sway-drawer 1 && ok "app_grid = true: a grade sobe" || fail "a grade não subiu"
exit $failed
