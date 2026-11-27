#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# O app Tema do Desktop (Rust e Python) com o adw-gtk3-dark de um GTK antes do 4.20 (o falso do lib-dark-text.sh,
# que o Rust troca pelo Adwaita-dark embutido). Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório
# em /repo, os binários em /bin e uma pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-theme-app:/bin/swaydesk-theme-app:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-theme-style.sh [rust|python]
#
# Os casos:
# - o botão marcado do Estilo (Claro/Escuro) é pintado na cor de destaque. O app abre duas vezes, uma com o tema
#   escuro (o Escuro marcado) e outra com o claro (o Claro marcado): o que é destaque num print e não no outro é o
#   botão que mudou. No Adwaita-dark, o marcado é só um cinza mais escuro, e nada muda de cor;
# - só no Rust: com a troca pelo Adwaita-dark, os botões têm o negrito do adw-gtk3 (o button { font-weight: bold }
#   dele). O mesmo app com o Adwaita-dark de verdade (sem a troca) tem o peso normal dele: mais tinta clara no print
#   com a troca. O GTK 3 (a versão em Python) lê o gtk-3.0 do adw-gtk3, que já tem o negrito.
# Sai com 1 se algum caso falhar.
set -u
. /repo/rust/scripts/lib-wait.sh
impl=${1:-rust}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do cp "$f" "$HOME/.local/bin/"; done
. /repo/rust/scripts/lib-dark-text.sh
fake_adw_gtk3_dark
# A família adw-gtk3 com as duas variantes (o app acha os temas pelo gtk-3.0): o Estilo fica ligado
for name in adw-gtk3 adw-gtk3-dark; do
  mkdir -p "$HOME/.themes/$name/gtk-3.0"
  bg="#fafafb" fg="#303030"
  [ "$name" = adw-gtk3-dark ] && bg="#1e1e1f" fg="#ffffff"
  printf '@define-color theme_bg_color %s;\n@define-color theme_fg_color %s;\n' "$bg" "$fg" > "$HOME/.themes/$name/gtk-3.0/gtk.css"
done
mkdir -p "$HOME/.themes/adw-gtk3/gtk-4.0" && cp "$HOME/.themes/adw-gtk3-dark/gtk-4.0/"*.css "$HOME/.themes/adw-gtk3/gtk-4.0/"
# Um destaque que nada mais na tela tem
accent="#d02090"

export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
eval "$(dbus-daemon --session --fork --print-address=1 | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
windows() { swaymsg -t get_tree | grep -c '"app_id": "org.swaydesktop.Theme"'; }
mkdir -p /tmp/no-system-icons
# shot NOME TEMA [TEMA_DO_GTK]: abre o app com o tema no config e o do GTK no settings.ini, tira o print e fecha
shot() {
  printf 'gtk_theme = "%s"\naccent = "%s"\naccent_auto = false\n' "$2" "$accent" > "$HOME/.config/swaydesk/config.toml"
  printf '[Settings]\ngtk-theme-name=%s\ngtk-application-prefer-dark-theme=1\n' "${3:-adw-gtk3-dark}" > "$HOME/.config/gtk-4.0/settings.ini"
  env G_DEBUG=fatal-criticals SWAY_SYSTEM_ICONS=/tmp/no-system-icons SWAY_DESKTOP_SHARE=/repo/home/.local/share/sway-desktop \
    SWAY_THEME_APP_IMPL="$impl" "$HOME/.local/bin/sway-theme-app" >> /tmp/app.log 2>&1 &
  local pid=$!
  for _ in $(seq $ticks); do [ "$(windows)" = 1 ] && break; sleep 0.1; done
  sleep 2
  grim -t ppm "/tmp/$1.ppm"
  grim "/out/theme-style-$impl-$1.png" 2>/dev/null
  swaymsg -q '[app_id="org.swaydesktop.Theme"] kill'
  for _ in $(seq $ticks); do [ "$(windows)" = 0 ] && break; sleep 0.1; done
  kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
}
# measure A B: os pixels na cor de destaque só em A, só em B; e a tinta clara (o texto) de cada um
measure() {
  python3 - "$accent" "/tmp/$1.ppm" "/tmp/$2.ppm" <<'PY'
import re, sys
def load(path):
    data = open(path, "rb").read()
    m = re.match(rb"P6\s+(\d+)\s+(\d+)\s+(\d+)\s", data)
    return int(m[1]), int(m[2]), data[m.end():]
a = tuple(int(sys.argv[1][i:i + 2], 16) for i in (1, 3, 5))
(w, h, p), (_, _, q) = load(sys.argv[2]), load(sys.argv[3])
near = lambda px, i: all(abs(px[i + k] - a[k]) <= 12 for k in range(3))
bright = lambda px, i: px[i] > 200 and px[i + 1] > 200 and px[i + 2] > 200
only_a = only_b = ink_a = ink_b = 0
for i in range(0, w * h * 3, 3):
    na, nb = near(p, i), near(q, i)
    only_a += na and not nb
    only_b += nb and not na
    ink_a += bright(p, i)
    ink_b += bright(q, i)
print(only_a, only_b, ink_a, ink_b)
PY
}

shot dark adw-gtk3-dark
shot light adw-gtk3
read -r only_dark only_light _ _ <<< "$(measure dark light)"
echo "destaque só no print do Escuro: $only_dark; só no do Claro: $only_light"
if [ "${only_dark:-0}" -ge 600 ] && [ "${only_light:-0}" -ge 600 ]; then
  ok "o botão marcado do Estilo fica na cor de destaque (Escuro e Claro)"
else
  fail "o botão marcado do Estilo não está na cor de destaque"
fi

if [ "$impl" = rust ]; then
  shot adwaita adw-gtk3-dark Adwaita-dark
  read -r _ _ ink_stand_in ink_adwaita <<< "$(measure dark adwaita)"
  echo "tinta clara com a troca: $ink_stand_in; com o Adwaita-dark de verdade: $ink_adwaita"
  # O print é o mesmo a cada rodada (sem a regra, deu igual, pixel a pixel): os rótulos dos botões em negrito dão
  # umas centenas de pixels a mais
  if [ "${ink_stand_in:-0}" -gt $(( ${ink_adwaita:-0} + 100 )) ]; then
    ok "com a troca pelo Adwaita-dark, os botões têm o negrito do adw-gtk3"
  else
    fail "com a troca pelo Adwaita-dark, os botões não têm o negrito do adw-gtk3"
  fi
fi

grep -q "CRITICAL\|Traceback\|panicked" /tmp/app.log && { fail "avisos no log"; cat /tmp/app.log; }
exit $failed
