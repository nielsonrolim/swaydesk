#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): rode com outro UID (-u 64000:64000), como o run-all-tests.sh.
# As janelas comuns do Swaydesk (Telas e Ajuda) ficam escuras com o adw-gtk3-dark num GTK antes do 4.20 (o Debian
# 13 tem o 4.18), num Sway headless. O CSS de GTK 4 do adw-gtk3 6.5 é o mesmo nas duas variantes e só escurece
# dentro de `@media (prefers-color-scheme: dark)`, que o GTK só lê a partir do 4.20: um tema falso em ~/.themes
# com essa forma (claro fora do @media, escuro dentro), escolhido no settings.ini com o prefer-dark, faz a janela
# clara sem a troca pelo Adwaita-dark embutido (swaydesk_ui::theme_stand_in). Conta, no print de cada janela, os
# pixels na cor clara do tema falso: quase nenhum passa. E com o adw-gtk3 claro, a janela fica clara (o Adwaita).
# E com a janela aberta, uma troca do gtk-theme e do color-scheme da sessão (org.gnome.desktop.interface, como o
# sway-theme grava): a janela segue (swaydesk_ui::follow_session_theme), no claro, no escuro e num tema de fora.
# Roda dentro do contêiner swaydesk-smoke:debian13-gsettings (o smoke com os schemas e o gsettings), com o
# repositório em /repo, os binários em /bin e uma pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 6g --memory-swap 6g -u 64000:64000 -e HOME=/tmp -v <passwd com o 64000>:/etc/passwd:ro \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-displays:/bin/swaydesk-displays:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-help:/bin/swaydesk-help:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-calendar:/bin/swaydesk-calendar:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13-gsettings /repo/rust/scripts/headless-dark-windows.sh
#
# Sai com 1 se algum caso falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg XDG_CONFIG_HOME=/tmp/home/.config
mkdir -p "$HOME/.config/swaydesk" "$HOME/.config/gtk-4.0" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8 GTK_A11Y=none
printf 'output HEADLESS-1 resolution 1280x800\ndefault_border none\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /out/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
eval "$(dbus-daemon --session --fork --print-address=1 | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }

# O tema falso, nas duas variantes, como o adw-gtk3 6.5: o mesmo CSS, claro fora do @media e escuro dentro
LIGHT=#fafafb
for name in adw-gtk3 adw-gtk3-dark; do
  mkdir -p "$HOME/.themes/$name/gtk-4.0"
  cat > "$HOME/.themes/$name/gtk-4.0/gtk.css" <<EOF
* { color: #ff00ff; }
window, window.background, .background { background-color: $LIGHT; color: #ff00ff; }
@media (prefers-color-scheme: dark) {
  * { color: #ffffff; }
  window, window.background, .background { background-color: #1e1e1f; color: #ffffff; }
}
EOF
  # O gtk-dark.css (o que o GTK lê com o prefer-dark) é o mesmo arquivo, como no adw-gtk3
  cp "$HOME/.themes/$name/gtk-4.0/gtk.css" "$HOME/.themes/$name/gtk-4.0/gtk-dark.css"
done

# Quantos pixels do print têm a cor exata (±3 em cada canal), em milésimos do print
share() {
  python3 - "$1" "$2" <<'EOF'
import sys
data = open(sys.argv[1], "rb").read()
parts = data.split(maxsplit=4)
w, h, pixels = int(parts[1]), int(parts[2]), parts[4]
want = bytes.fromhex(sys.argv[2].lstrip("#"))
n = sum(1 for i in range(0, w * h * 3, 3) if all(abs(pixels[i + c] - want[c]) <= 3 for c in range(3)))
print(n * 1000 // (w * h))
EOF
}

# Abre o app, espera a janela, tira o print e fecha
shot() {  # binário, app_id, nome do print
  "/bin/$1" >> /out/app.log 2>&1 &
  local app=$!
  for _ in $(seq 100); do
    swaymsg -t get_tree | grep -q "\"app_id\": \"$2\"" && break
    sleep 0.1
  done
  sleep 1.5
  grim -t ppm "/out/$3.ppm"
  grim "/out/$3.png" 2>/dev/null
  kill $app 2>/dev/null; wait $app 2>/dev/null
}

# A luminância média do print (0 a 255)
lum() {
  python3 - "$1" <<'EOF'
import sys
data = open(sys.argv[1], "rb").read()
parts = data.split(maxsplit=4)
w, h, pixels = int(parts[1]), int(parts[2]), parts[4]
print(sum(pixels[0:w * h * 3]) // (w * h * 3))
EOF
}

# A sessão como o sway-theme a grava: o settings.ini (o GTK 4 o lê ao abrir) e o org.gnome.desktop.interface
settings() {  # tema, prefer-dark do settings.ini (0 ou 1), color-scheme
  printf '[Settings]\ngtk-theme-name=%s\ngtk-application-prefer-dark-theme=%s\n' "$1" "$2" > "$HOME/.config/gtk-4.0/settings.ini"
  gsettings set org.gnome.desktop.interface gtk-theme "$1"
  gsettings set org.gnome.desktop.interface color-scheme "$3"
}
command -v gsettings > /dev/null || { fail "sem o gsettings: rode na imagem swaydesk-smoke:debian13-gsettings"; exit 1; }

# 1. adw-gtk3-dark, com e sem o prefer-dark no settings.ini: escuras (no máximo 2% da cor clara do tema falso)
for prefer in 1 0; do
  settings adw-gtk3-dark $prefer prefer-dark
  for app in "swaydesk-displays org.swaydesktop.Displays displays" "swaydesk-help org.swaydesktop.Help help"; do
    set -- $app
    shot "$1" "$2" "dark-$3-$prefer"
    n=$(share "/out/dark-$3-$prefer.ppm" $LIGHT)
    [ "$n" -le 20 ] && ok "$3: escura com o adw-gtk3-dark, prefer-dark $prefer ($n‰ claros)" \
      || fail "$3: clara com o adw-gtk3-dark, prefer-dark $prefer ($n‰ na cor clara do tema: sem a troca pelo Adwaita-dark?)"
  done
done

# 2. O adw-gtk3 claro continua claro: a troca é pelo Adwaita, não pelo escuro (o fundo claro do Adwaita, não o do falso)
settings adw-gtk3 0 default
shot swaydesk-help org.swaydesktop.Help help-light
light=$(lum /out/help-light.ppm)
[ "$light" -ge 150 ] && ok "help: clara com o adw-gtk3 (luminância média $light)" || fail "help: escura com o adw-gtk3 (luminância média $light)"

# 3. Com a janela aberta, a troca da sessão (o Claro e o Escuro do app Tema, e um tema de fora): a janela segue, sem
#    reabrir. O settings.ini fica no escuro de quando ela abriu: o GTK 4.18 só o lê ao abrir
for app in "swaydesk-displays org.swaydesktop.Displays displays" "swaydesk-help org.swaydesktop.Help help"; do
  set -- $app
  settings adw-gtk3-dark 1 prefer-dark
  "/bin/$1" >> /out/app.log 2>&1 &
  pid=$!
  for _ in $(seq 100); do
    swaymsg -t get_tree | grep -q "\"app_id\": \"$2\"" && break
    sleep 0.1
  done
  sleep 1.5
  # tema, color-scheme, o que se espera
  for step in "adw-gtk3 default clara" "adw-gtk3-dark prefer-dark escura" "HighContrast default clara"; do
    set -- $1 $2 $3 $step
    gsettings set org.gnome.desktop.interface gtk-theme "$4"
    gsettings set org.gnome.desktop.interface color-scheme "$5"
    sleep 1.5
    grim -t ppm "/out/live-$3-$4.ppm"
    grim "/out/live-$3-$4.png" 2>/dev/null
    l=$(lum "/out/live-$3-$4.ppm")
    if [ "$6" = clara ]; then
      [ "$l" -ge 150 ] && ok "$3 aberta: clara depois do $4 (luminância $l)" || fail "$3 aberta: não ficou clara depois do $4 (luminância $l)"
    else
      [ "$l" -le 100 ] && ok "$3 aberta: escura depois do $4 (luminância $l)" || fail "$3 aberta: não ficou escura depois do $4 (luminância $l)"
    fi
  done
  kill $pid 2>/dev/null; wait $pid 2>/dev/null
done

# 4. As superfícies sempre escuras (os popups da barra, como o calendário): com o adw-gtk3-dark, o texto não tem a
#    cor do tema claro (o magenta do falso, fora do @media). Os cartões do popup são escuros; com o texto do tema
#    claro, os títulos ficavam pretos neles (o popup das notificações no vídeo)
magenta() {  # pixels quase magenta, em milésimos
  python3 - "$1" <<'EOF'
import sys
data = open(sys.argv[1], "rb").read()
parts = data.split(maxsplit=4)
w, h, p = int(parts[1]), int(parts[2]), parts[4]
n = sum(1 for i in range(0, w * h * 3, 3) if p[i] > 150 and p[i + 1] < 100 and p[i + 2] > 150)
print(n * 10000 // (w * h))
EOF
}
settings adw-gtk3-dark 1 prefer-dark
/bin/swaydesk-calendar >> /out/app.log 2>&1 &
pid=$!
for _ in $(seq 50); do [ -e "$XDG_RUNTIME_DIR/waybar-calendar.pid" ] && break; sleep 0.1; done
sleep 1.5
grim -t ppm /out/calendar.ppm
grim /out/calendar.png 2>/dev/null
kill $pid 2>/dev/null; wait $pid 2>/dev/null
n=$(magenta /out/calendar.ppm)
[ "$n" -eq 0 ] && ok "calendário: o texto não tem a cor do tema claro" \
  || fail "calendário: $n‱ do print na cor do texto do tema claro (o popup sem a troca pelo Adwaita-dark)"

rm -f /out/*.ppm
[ "$failed" = 0 ] || exit 1
