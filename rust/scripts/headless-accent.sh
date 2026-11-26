#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): rode com outro UID (-u 64000:64000), como o run-all-tests.sh.
# A cor de destaque nos widgets do GTK 4 dos apps do Swaydesk (o interruptor "Ligada" do app Telas), num Sway
# headless, com a cor da config em vermelho (#e01b24), longe do azul do GTK:
#   1. sem o ~/.config/gtk-4.0/gtk.css, como numa máquina com outro desktop (o app Tema não o grava): o
#      config::accent_css que o app carrega dá a cor;
#   2. com o gtk.css que o app Tema grava (só o Swaydesk): a mesma cor;
#   3. com um gtk.css escrito à mão com o interruptor em verde: o do usuário vence (prioridade USER acima da
#      APPLICATION do app). O tema Default do GTK 4.18 não lê as variáveis (--accent-bg-color), só regras.
# Conta, no print da região do interruptor, os pixels de cada cor exata. E o Aplicar, desligado ao abrir: as regras do
# destaque (prioridade APPLICATION) não podem apagar o estado desligado do tema.
# Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório em /repo, o binário em /bin e uma pasta
# gravável em /out (as capturas):
#
#   docker run --rm --memory 6g --memory-swap 6g -u 64000:64000 -e HOME=/tmp -v <passwd com o 64000>:/etc/passwd:ro \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-displays:/bin/swaydesk-displays:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-accent.sh
#
# Sai com 1 se algum caso falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg XDG_CONFIG_HOME=/tmp/home/.config
mkdir -p "$HOME/.config/swaydesk" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8 GTK_A11Y=none
printf 'output HEADLESS-1 resolution 1280x800\ndefault_border none\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /out/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
eval "$(dbus-daemon --session --fork --print-address=1 | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"
printf 'accent = "#e01b24"\naccent_auto = false\n' > "$HOME/.config/swaydesk/config.toml"

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }

# Quantos pixels do print têm a cor exata (±3 em cada canal)
count() {
  python3 - "$1" "$2" <<'EOF'
import sys
data = open(sys.argv[1], "rb").read()
# PPM binário: P6, largura altura, máximo, e os bytes
parts = data.split(maxsplit=4)
w, h, pixels = int(parts[1]), int(parts[2]), parts[4]
want = bytes.fromhex(sys.argv[2].lstrip("#"))
n = 0
for i in range(0, w * h * 3, 3):
    if all(abs(pixels[i + c] - want[c]) <= 3 for c in range(3)):
        n += 1
print(n)
EOF
}

# Abre o app, espera a janela, tira o print e fecha
shot() {
  /bin/swaydesk-displays >> /out/app.log 2>&1 &
  local app=$!
  for _ in $(seq 100); do
    swaymsg -t get_tree | grep -q '"app_id": "org.swaydesktop.Displays"' && break
    sleep 0.1
  done
  sleep 1.5
  # Só a região do interruptor (o Aplicar, também no destaque, não conta)
  grim -g "$SWITCH" -t ppm "/out/$1.ppm"
  grim -g "$APPLY" -t ppm "/out/$1-apply.ppm"
  grim "/out/$1.png" 2>/dev/null
  kill $app 2>/dev/null; wait $app 2>/dev/null
}

RED=#e01b24 GREEN=#00ff00
# O interruptor "Ligada", à direita da lista das telas, na janela de 1280 × 800; ligado, tem uns 494 pixels da cor
SWITCH="1200,525 80x40" ENOUGH=300
# O Aplicar, embaixo à direita: desligado ao abrir (nada mudou), com a cor do tema para desligado, sem o destaque
APPLY="1190,748 80x40" FEW=20

# 1. Sem gtk.css (outro desktop instalado)
rm -rf "$HOME/.config/gtk-4.0"
shot accent-1-no-gtk-css
n=$(count /out/accent-1-no-gtk-css.ppm $RED)
[ "$n" -ge $ENOUGH ] && ok "sem o gtk.css, o interruptor fica na cor de destaque ($n pixels)" \
  || fail "sem o gtk.css, só $n pixels na cor de destaque (o interruptor ficou no azul do GTK?)"
n=$(count /out/accent-1-no-gtk-css-apply.ppm $RED)
[ "$n" -le $FEW ] && ok "o Aplicar desligado não fica na cor de destaque ($n pixels)" \
  || fail "o Aplicar desligado tem $n pixels na cor de destaque: parece ligado"

# 2. Com o gtk.css que o app Tema grava
mkdir -p "$HOME/.config/gtk-4.0"
printf '/* Generated: a cor de destaque do app Tema */\n:root {\n  --accent-bg-color: %s;\n  --accent-fg-color: #ffffff;\n}\n@define-color accent_bg_color %s;\n@define-color accent_fg_color #ffffff;\n' \
  $RED $RED > "$HOME/.config/gtk-4.0/gtk.css"
shot accent-2-gtk-css
n=$(count /out/accent-2-gtk-css.ppm $RED)
[ "$n" -ge $ENOUGH ] && ok "com o gtk.css do app Tema, a mesma cor ($n pixels)" || fail "com o gtk.css do app Tema, só $n pixels"

# 3. Um gtk.css do usuário com o interruptor em verde: vence o do app
printf 'switch:checked { background-color: %s; border-color: %s; }\n' $GREEN $GREEN > "$HOME/.config/gtk-4.0/gtk.css"
shot accent-3-user-css
n=$(count /out/accent-3-user-css.ppm $GREEN)
[ "$n" -ge $ENOUGH ] && ok "o gtk.css do usuário vence ($n pixels em verde)" || fail "o gtk.css do usuário não venceu: $n pixels em verde"

rm -f /out/*.ppm
[ "$failed" = 0 ] || exit 1
