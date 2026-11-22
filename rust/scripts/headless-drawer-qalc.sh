#!/bin/bash
# Testa a calculadora da grade de apps com o qalc (Rust ou Python), num Sway headless. Roda dentro de um
# contêiner swaydesk-smoke:debian13 com o qalc e o wl-clipboard instalados, com o repositório em /repo, o
# binário da grade em /bin e uma pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) --group-add $(getent group render | cut -d: -f3) --device /dev/dri \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-drawer:/bin/swaydesk-drawer:ro \
#     -v /usr/share/icons:/usr/share/icons:ro -v /tmp/out:/out <imagem com qalc> \
#     /repo/rust/scripts/headless-drawer-qalc.sh [rust|python]
#
# Os casos: "5 km to mi" mostra o resultado do qalc e o Enter o copia; "2+3" usa a conta simples; um nome de
# app não chama o qalc e mantém a lista. O qalc falso no PATH anota cada chamada e repassa ao de verdade. Na
# versão em Python, o Enter não chega à busca no headless (também na main): a cópia só é conferida no Rust.
# Sai com 1 se algum caso falhar.
set -u
impl=${1:-rust}
if ! command -v qalc >/dev/null || ! command -v wl-paste >/dev/null; then
  echo "FALHOU  o contêiner precisa do qalc e do wl-clipboard"
  exit 1
fi
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/sway-desktop" "$HOME/.local/share/applications" /tmp/fake \
  && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do cp "$f" "$HOME/.local/bin/"; done
real_qalc=$(command -v qalc)
cat > /tmp/fake/qalc <<SCRIPT
#!/bin/sh
printf '%s\n' "\$*" >> /tmp/qalc-calls
exec $real_qalc "\$@"
SCRIPT
chmod +x /tmp/fake/qalc
export PATH=/tmp/fake:$PATH
cat > "$HOME/.local/share/applications/firefox.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Firefox
Exec=true
Icon=utilities-terminal
DESKTOP
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
# Sem um teclado vivo, cada wtype cria o seu, e o primeiro caractere se perde
wtype -s 3600000 -k F24 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
shown() { [ -e "$XDG_RUNTIME_DIR/sway-drawer.shown" ]; }
close_grid() {
  # Com texto na busca, o primeiro Esc pode só limpá-la
  for _ in 1 2 3; do
    shown || break
    wtype -k Escape
    for _ in $(seq 10); do shown || break; sleep 0.1; done
  done
  sleep 0.5
}
copy_checked() { [ "$impl" = rust ]; }
open_grid() {
  close_grid
  SWAY_DRAWER_IMPL=$impl "$HOME/.local/bin/sway-drawer"
  for _ in $(seq 30); do shown && break; sleep 0.1; done
  sleep 1
}

SWAY_DRAWER_IMPL=$impl "$HOME/.local/bin/sway-drawer" start > /tmp/drawer.log 2>&1 &
sleep 2

# Um nome de app: a lista fica, e o qalc não é chamado
open_grid
shown && ok "a grade abre" || { fail "a grade não abriu"; cat /tmp/drawer.log; exit 1; }
rm -f /tmp/qalc-calls
wtype firefox
sleep 1.5
grim "/out/$impl-1-app.png" 2>/dev/null
[ -e /tmp/qalc-calls ] && fail "um nome de app chamou o qalc: $(cat /tmp/qalc-calls)" || ok "um nome de app não chama o qalc"
close_grid

# A conta simples: sem o qalc
open_grid
wtype '2+3'
sleep 1
grim "/out/$impl-2-conta.png" 2>/dev/null
[ -e /tmp/qalc-calls ] && fail "a conta simples chamou o qalc" || ok "a conta simples não chama o qalc"
if copy_checked; then
  wtype -k Return
  sleep 0.8
  [ "$(wl-paste -n 2>/dev/null)" = 5 ] && ok "o Enter copia 5" || fail "a conta copiou '$(wl-paste -n 2>/dev/null)'"
fi

# A conversão: o qalc, com a expressão depois do --
open_grid
wtype '5 km to mi'
sleep 2
grim "/out/$impl-3-qalc.png" 2>/dev/null
calls=$(cat /tmp/qalc-calls 2>/dev/null)
case $calls in
  *"-- 5 km to mi"*) ok "o qalc recebe a expressão depois do --" ;;
  *) fail "chamadas do qalc: '$calls'" ;;
esac
n=$(grep -c . /tmp/qalc-calls 2>/dev/null) || n=0
[ "$n" -ge 1 ] && [ "$n" -le 3 ] && ok "a espera junta as teclas ($n chamadas)" || fail "o qalc foi chamado $n vezes"
if copy_checked; then
  wtype -k Return
  sleep 0.8
  copied=$(wl-paste -n 2>/dev/null)
  case $copied in
    *mi*) ok "o Enter copia o resultado ($copied)" ;;
    *) fail "o Enter copiou '$copied'" ;;
  esac
  shown && fail "a grade continuou aberta depois do Enter" || ok "a grade se esconde depois do Enter"
fi
[ "$failed" = 0 ] || { echo "--- registro da grade"; cat /tmp/drawer.log; }
exit "$failed"
