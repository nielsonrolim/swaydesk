#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# O switch ligado com um tema que o desenha numa imagem, como o Mint-Y (a pílula num PNG de 52×24, sem raio, cantos
# transparentes), no app Tema do Desktop e no de Monitores. Roda dentro do contêiner swaydesk-smoke:debian13, com o
# repositório em /repo, os binários em /bin e uma pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-theme-app:/bin/swaydesk-theme-app:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-displays:/bin/swaydesk-displays:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-image-switch.sh
#
# O caso: a pílula do tema (cinza, #808080) aparece inteira, e os cantos da caixa dela são o fundo da janela
# (#303030). Antes, o fundo do destaque pintava o retângulo do switch atrás da imagem. Só a versão em Rust: a em
# Python, com este tema falso, não liga o switch (o tests/test_accent.py cobre o SWITCH_CSS dela). Sai com 1 se algum
# caso falhar.
set -u
impl=rust
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" "$HOME/.config/gtk-3.0" "$HOME/.config/gtk-4.0" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do cp "$f" "$HOME/.local/bin/"; done
# Um destaque que nada mais na tela tem
accent="#d02090"
theme=Mint-Y-Dark-Orange
for gtk in gtk-3.0 gtk-4.0; do
  dir="$HOME/.themes/$theme/$gtk"
  mkdir -p "$dir"
  python3 - "$dir/switch.png" <<'PY'
import struct, sys, zlib
w, h, r = 52, 24, 12
rows = b""
for y in range(h):
    rows += b"\0"
    for x in range(w):
        cx = min(max(x + 0.5, r), w - r)
        inside = (x + 0.5 - cx) ** 2 + (y + 0.5 - r) ** 2 <= r * r
        rows += bytes((128, 128, 128, 255)) if inside else bytes(4)
chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
open(sys.argv[1], "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
                              + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
PY
  # Como o Mint-Y: o switch e o botão sem borda, sem raio, a imagem no fundo
  slider="switch > slider"; [ "$gtk" = gtk-3.0 ] && slider="switch slider"
  cat > "$dir/gtk.css" <<CSS
@define-color theme_bg_color #303030;
@define-color theme_fg_color #ffffff;
window { background-color: #303030; color: #ffffff; }
switch { min-width: 52px; min-height: 24px; background-image: url("switch.png"); background-size: 52px 24px;
  background-repeat: no-repeat; background-position: center center; background-color: transparent; border: none;
  box-shadow: none; color: transparent; font-size: 1px; }
$slider { min-width: 1px; min-height: 1px; border: none; box-shadow: none; background: none; }
CSS
done
# O switch é o "Use the wallpaper's color", ligado (o accent_auto do config)
printf 'gtk_theme = "%s"\naccent = "%s"\naccent_auto = true\n' "$theme" "$accent" > "$HOME/.config/swaydesk/config.toml"
for gtk in gtk-3.0 gtk-4.0; do printf '[Settings]\ngtk-theme-name=%s\n' "$theme" > "$HOME/.config/$gtk/settings.ini"; done

export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
eval "$(dbus-daemon --session --fork --print-address=1 | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
windows() { swaymsg -t get_tree | grep -c "\"app_id\": \"$1\""; }
mkdir -p /tmp/no-system-icons
# check NOME LANÇADOR APP_ID VARIÁVEL_DO_IMPL
check() {
  env G_DEBUG=fatal-criticals SWAY_SYSTEM_ICONS=/tmp/no-system-icons SWAY_DESKTOP_SHARE=/repo/home/.local/share/sway-desktop \
    "$4=$impl" "$HOME/.local/bin/$2" >> /tmp/app.log 2>&1 &
  local pid=$!
  for _ in $(seq 100); do [ "$(windows "$3")" = 1 ] && break; sleep 0.1; done
  sleep 3
  grim -t ppm "/tmp/$1.ppm"
  grim "/out/image-switch-$impl-$1.png" 2>/dev/null
  swaymsg -q "[app_id=\"$3\"] kill"
  for _ in $(seq 50); do [ "$(windows "$3")" = 0 ] && break; sleep 0.1; done
  kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
  # Os pixels da pílula cinza e quantos dos 16 dos cantos da caixa dela não são o fundo da janela
  read -r pill corners <<< "$(python3 - "/tmp/$1.ppm" <<'PY'
import re, sys
data = open(sys.argv[1], "rb").read()
m = re.match(rb"P6\s+(\d+)\s+(\d+)\s+(\d+)\s", data)
w, h, p = int(m[1]), int(m[2]), data[m.end():]
px = lambda x, y: p[(y * w + x) * 3:(y * w + x) * 3 + 3]
grey = {(x, y) for y in range(h) for x in range(w) if all(abs(c - 128) <= 3 for c in px(x, y))}
# A pílula: a maior mancha cinza contínua (outros cinzas da janela, como bordas, ficam de fora)
best, seen = [], set()
for start in grey:
    if start in seen:
        continue
    seen.add(start)
    blob, todo = [], [start]
    while todo:
        x, y = todo.pop()
        blob.append((x, y))
        for n in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
            if n in grey and n not in seen:
                seen.add(n)
                todo.append(n)
    if len(blob) > len(best):
        best = blob
grey = best
if not grey:
    print(0, 16)
    sys.exit()
x0, x1 = min(x for x, _ in grey), max(x for x, _ in grey)
y0, y1 = min(y for _, y in grey), max(y for _, y in grey)
# O fundo da janela do tema (#303030): o que não é ele num canto é a caixa
off = lambda c: any(abs(c[k] - 48) > 12 for k in range(3))
print(len(grey), sum(off(px(x, y)) for x in (x0, x0 + 1, x1 - 1, x1) for y in (y0, y0 + 1, y1 - 1, y1)))
PY
)"
  echo "$1: pílula cinza de $pill pixels; $corners dos 16 cantos fora do fundo da janela"
  if [ "${pill:-0}" -ge 400 ] && [ "${pill:-0}" -le 1300 ] && [ "${corners:-16}" = 0 ]; then
    ok "$1: a pílula do tema, sem uma caixa do destaque"
  else
    fail "$1: o destaque faz uma caixa em volta da pílula do tema, ou apaga a imagem"
  fi
}

check theme sway-theme-app org.swaydesktop.Theme SWAY_THEME_APP_IMPL
check displays sway-displays org.swaydesktop.Displays SWAY_DISPLAYS_IMPL

grep -q "CRITICAL\|Traceback\|panicked" /tmp/app.log && { fail "avisos no log"; cat /tmp/app.log; }
exit $failed
