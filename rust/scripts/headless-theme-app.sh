#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o app Tema do Desktop (Rust e Python) num Sway headless, com o ponteiro virtual (swaydesk-vpointer).
# Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório em /repo, os binários em /bin e uma
# pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-theme-app:/bin/swaydesk-theme-app:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-theme-app.sh [rust|python]
#
# Temas falsos em ~/.themes: a família Mint-Y (escuro teal e azul, claro teal) e o Arc-Dark, de fora. Os
# casos: o app sobe com a versão pedida e o app_id certo; abrir de novo não abre outra janela; escolher o
# Arc-Dark na lateral e Aplicar grava o tema no config.toml, e a janela continua aberta. A cor do papel de parede
# (um vermelho liso, só na versão em Rust): o interruptor abre ligado (o padrão, sem a chave), e o Aplicar grava o vermelho com
# accent_auto = true; uma amostra (o azul) desliga o interruptor, e o Aplicar grava a cor com accent_auto = false;
# ligar de novo volta ao vermelho. Sai com 1 se algum caso falhar.
set -u
impl=${1:-rust}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" "$HOME/.local/share/wallpaper" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
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
# Dois temas de ícones para a lista, com uma pasta cada (sem o Papirus: os casos de antes ficam como eram)
icons() {  # nome, cor da pasta
  mkdir -p "$HOME/.icons/$1/48x48/places"
  printf '[Icon Theme]\nName=%s\nDirectories=48x48/places\n\n[48x48/places]\nSize=48\nContext=Places\n' "$1" > "$HOME/.icons/$1/index.theme"
  printf '<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48"><rect x="4" y="10" width="40" height="30" fill="%s"/></svg>\n' "$2" \
    > "$HOME/.icons/$1/48x48/places/folder.svg"
}
icons Adwaita "#3584e4"
icons Yaru "#e95420"
printf 'gtk_theme = "Mint-Y-Dark-Teal"\n' > "$HOME/.config/swaydesk/config.toml"
# O papel de parede: um PPM vermelho (#e25252, o red do Papirus) com o nome de sempre; o GdkPixbuf lê pelo conteúdo
python3 -c "import sys; sys.stdout.buffer.write(b'P6\n64 64\n255\n' + bytes([0xe2, 0x52, 0x52]) * 4096)" \
  > "$HOME/.local/share/wallpaper/current.jpg"

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
# A lista de ícones só com os dois falsos: o tests/run-all-tests.sh monta o /usr/share/icons do host, e com o
# Papirus lá a lista ganha a linha da família no topo, e o clique na primeira linha cai nela
mkdir -p /tmp/no-system-icons
app() { env G_DEBUG=fatal-criticals SWAY_SYSTEM_ICONS=/tmp/no-system-icons SWAY_DESKTOP_SHARE=/repo/home/.local/share/sway-desktop SWAY_THEME_APP_IMPL=$impl "$HOME/.local/bin/sway-theme-app" >> /tmp/app.log 2>&1 & }

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
for _ in $(seq 100); do grep -q 'gtk_theme = "Arc-Dark"' "$HOME/.config/swaydesk/config.toml" && break; sleep 0.1; done
sleep 1
shot 3-aplicado
if grep -q 'gtk_theme = "Arc-Dark"' "$HOME/.config/swaydesk/config.toml"; then
  ok "Aplicar grava o tema no config.toml"
else
  fail "o tema não foi gravado: $(cat "$HOME/.config/swaydesk/config.toml")"
fi
[ "$(windows)" = 1 ] && ok "a janela continua aberta depois de aplicar" || fail "a janela fechou ao aplicar"

config() { cat "$HOME/.config/swaydesk/config.toml"; }
# Os casos da cor do papel só na versão em Rust: os cliques são posições na janela, e o layout do GTK 3 é outro (o
# app em Python é testado na VM)
if [ "$impl" = rust ]; then
# O interruptor abriu ligado: o Arc-Dark foi com a cor do papel
if config | grep -qx 'accent = "#e25252"' && config | grep -qx 'accent_auto = true'; then
  ok "com o interruptor ligado (o padrão), o Aplicar grava a cor do papel e accent_auto = true"
else
  fail "a cor do papel não foi gravada: $(config)"
fi
# A barra de aviso do Aplicar (no contêiner, o dconf falha) empurra o conteúdo para baixo: o X a fecha
close_info() { pointer move $((wx + ww - 23)) $((wy + 67)) sleep 100 click sleep 300; }
apply_and_wait() {  # o Aplicar; espera a linha no config.toml
  pointer move $((wx + ww - 171)) $((wy + 22)) sleep 100 click sleep 100
  for _ in $(seq 100); do config | grep -qx "$1" && break; sleep 0.1; done
  sleep 1
  close_info
}
close_info
# O azul é a terceira amostra da primeira linha
pointer move $((wx + 345)) $((wy + 472)) sleep 100 click sleep 500
shot 4-azul
apply_and_wait 'accent_auto = false'
shot 5-azul-aplicado
if config | grep -qx 'accent = "#5294e2"' && config | grep -qx 'accent_auto = false'; then
  ok "escolher uma cor desliga o interruptor, e o Aplicar grava accent_auto = false"
else
  fail "a cor à mão não desligou o modo: $(config)"
fi
pointer move $((wx + 277)) $((wy + 432)) sleep 100 click sleep 500  # o interruptor
shot 6-auto-ligado
apply_and_wait 'accent_auto = true'
shot 7-auto-aplicado
if config | grep -qx 'accent = "#e25252"' && config | grep -qx 'accent_auto = true'; then
  ok "ligar o interruptor e Aplicar volta à cor do papel"
else
  fail "o interruptor não voltou à cor do papel: $(config)"
fi
# Reaberto no destaque padrão exato (que não é amostra), com o modo desligado: trocar só o tema mantém o padrão,
# em vez de gravar a amostra mais perto (darkcyan)
kill "$pid" 2>/dev/null
for _ in $(seq 50); do [ "$(windows)" = 0 ] && break; sleep 0.1; done
printf 'gtk_theme = "Mint-Y-Dark-Teal"\naccent = "#199ca8"\nfolder_color = ""\naccent_auto = false\n' > "$HOME/.config/swaydesk/config.toml"
app
for _ in $(seq 100); do [ "$(windows)" = 1 ] && break; sleep 0.1; done
sleep 1.5
pid=$(pgrep -n -f "swaydesk-theme-app")
shot 8-padrao
pointer move $((wx + 100)) $((wy + 235)) sleep 100 click sleep 500  # o Arc-Dark
apply_and_wait 'gtk_theme = "Arc-Dark"'
shot 9-padrao-aplicado
if config | grep -qx 'accent = "#199ca8"' && config | grep -qx 'folder_color = ""' && config | grep -qx 'accent_auto = false'; then
  ok "reaberto no destaque padrão, trocar o tema mantém #199ca8 e as pastas na padrão"
else
  fail "o destaque padrão não ficou: $(config)"
fi
# A lista de ícones (Adwaita e Yaru, os dois falsos; o em uso é o Yaru, que a troca de tema achou na reserva):
# abrir, escolher o Adwaita (a primeira linha) e Aplicar grava só os ícones
pointer move $((wx + 340)) $((wy + 355)) sleep 100 click sleep 600
shot 10-icones-lista
pointer move $((wx + 342)) $((wy + 405)) sleep 100 click sleep 500
apply_and_wait 'icon_theme = "Adwaita"'
shot 11-icones-aplicado
if config | grep -qx 'icon_theme = "Adwaita"' && config | grep -qx 'gtk_theme = "Arc-Dark"' && config | grep -qx 'accent = "#199ca8"' \
  && config | grep -qx 'accent_auto = false'; then
  ok "a lista de ícones troca só os ícones"
else
  fail "os ícones não foram gravados: $(config)"
fi
fi
grep -q "CRITICAL\|Traceback\|panicked" /tmp/app.log && { fail "avisos no log"; cat /tmp/app.log; }
kill "$pid" 2>/dev/null
exit $failed
