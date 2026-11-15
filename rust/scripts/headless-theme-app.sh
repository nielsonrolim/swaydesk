#!/bin/bash
# Testa o app Tema do Desktop (Rust e Python) num Sway headless, com o ponteiro virtual (swaydesk-vpointer).
# Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório em /repo, os binários em /bin e uma
# pasta gravável em /out (as capturas):
#
#   docker run --rm -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-theme-app:/bin/swaydesk-theme-app:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-theme-app.sh [rust|python]
#
# Temas falsos em ~/.themes: a família Mint-Y (escuro teal e azul, claro teal) e o Arc-Dark, de fora. Os
# casos: o app sobe com a versão pedida e o app_id certo; abrir de novo não abre outra janela; escolher o
# Arc-Dark na lateral e Aplicar grava o tema no config.toml, e a janela continua aberta; Esc fecha. Sai com 1
# se algum caso falhar.
set -u
impl=${1:-rust}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/sway-desktop" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
theme() {  # nome, fundo, texto, destaque
  mkdir -p "$HOME/.themes/$1/gtk-3.0"
  printf '@define-color theme_bg_color %s;\n@define-color theme_fg_color %s;\n@define-color theme_selected_bg_color %s;\n' \
    "$2" "$3" "$4" > "$HOME/.themes/$1/gtk-3.0/gtk.css"
}
theme Mint-Y-Dark-Teal "#2f2f2f" "#dadada" "#199ca8"
theme Mint-Y-Dark-Blue "#2f2f2f" "#dadada" "#0c75de"
theme Mint-Y-Teal "#f0f0f0" "#303030" "#199ca8"
theme Arc-Dark "#383c4a" "#d3dae3" "#5294e2"
printf 'gtk_theme = "Mint-Y-Dark-Teal"\n' > "$HOME/.config/sway-desktop/config.toml"

export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
# O app é único pelo D-Bus da sessão (GtkApplication): sem barramento, cada um abriria a sua janela
eval "$(dbus-daemon --session --fork --print-address=1 | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
pointer() { /bin/swaydesk-vpointer --size 1280x720 "$@"; }
shot() { grim "/out/theme-app-$impl-$1.png" 2>/dev/null; }
windows() { swaymsg -t get_tree | grep -c '"app_id": "org.swaydesktop.Theme"'; }
# Os modelos do sway-desktop-config: o binário copiado não está ao lado de um share/ (o install.sh liga)
app() { env G_DEBUG=fatal-criticals SWAY_DESKTOP_SHARE=/repo/home/.local/share/sway-desktop SWAY_THEME_APP_IMPL=$impl "$HOME/.local/bin/sway-theme-app" >> /tmp/app.log 2>&1 & }

app
for _ in $(seq 100); do [ "$(windows)" = 1 ] && break; sleep 0.1; done
sleep 1.5
pid=$(pgrep -n -f "swaydesk-theme-app|sway-theme-app")
if [ "$(windows)" != 1 ]; then
  fail "a janela não abriu"; cat /tmp/app.log; exit 1
fi
exe=$(readlink /proc/$pid/exe)
case "$impl:$exe" in rust:*swaydesk-theme-app|python:*python3*) ok "sobe com o app_id certo ($exe)" ;; *) fail "binário errado: $exe" ;; esac
shot 1-aberto
app; sleep 2
[ "$(windows)" = 1 ] && ok "abrir de novo não abre outra janela" || fail "abriu $(windows) janelas"

# A janela ocupa a tela (o Sway lado a lado): o Arc-Dark é a última linha da lateral
swaymsg -t get_tree > /tmp/tree.json
python3 - > /tmp/rect <<'PY'
import json
def find(n):
    if n.get("app_id") == "org.swaydesktop.Theme":
        return n
    for c in n.get("nodes", []) + n.get("floating_nodes", []):
        f = find(c)
        if f:
            return f
r = find(json.load(open("/tmp/tree.json")))["rect"]
print(r["x"], r["y"], r["width"], r["height"])
PY
read -r wx wy ww wh < /tmp/rect
echo "janela: $wx $wy $ww $wh"
# O Arc-Dark é a segunda linha da lateral, depois do título "Other themes" (a família Mint-Y vem antes,
# com o título "Themes")
pointer move $((wx + 100)) $((wy + 235)) sleep 100 click sleep 500
shot 2-arc
pointer move $((wx + ww - 171)) $((wy + 22)) sleep 100 click sleep 100  # o Aplicar, à esquerda dos botões da janela
for _ in $(seq 100); do grep -q 'gtk_theme = "Arc-Dark"' "$HOME/.config/sway-desktop/config.toml" && break; sleep 0.1; done
sleep 1
shot 3-aplicado
if grep -q 'gtk_theme = "Arc-Dark"' "$HOME/.config/sway-desktop/config.toml"; then
  ok "Aplicar grava o tema no config.toml"
else
  fail "o tema não foi gravado: $(cat "$HOME/.config/sway-desktop/config.toml")"
fi
[ "$(windows)" = 1 ] && ok "a janela continua aberta depois de aplicar" || fail "a janela fechou ao aplicar"
grep -q "CRITICAL\|Traceback\|panicked" /tmp/app.log && { fail "avisos no log"; cat /tmp/app.log; }
kill "$pid" 2>/dev/null
exit $failed
