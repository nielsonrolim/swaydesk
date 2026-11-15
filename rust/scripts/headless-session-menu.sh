#!/bin/bash
# Testa o menu da sessão (Rust e Python) num Sway headless, com o ponteiro virtual (swaydesk-vpointer).
# Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório em /repo, os binários em /bin e uma
# pasta gravável em /out (as capturas):
#
#   docker run --rm -u $(id -u):$(id -g) \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-session-menu:/bin/swaydesk-session-menu:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-session-menu.sh [rust|python]
#
# O session-action é trocado por um falso, que grava a ação. Os casos: o menu sobe com a versão pedida;
# clicar fora fecha; abrir de novo fecha; clicar num botão fecha o menu e roda a ação, numa sessão
# própria; um CRITICAL do GTK derruba o menu (G_DEBUG). O teclado (letras, setas, Esc) fica para a VM:
# o headless não tem teclado virtual. Sai com 1 se algum caso falhar.
set -u
impl=${1:-rust}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/sway-desktop" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
rm -f "$HOME/.local/bin/session-action"
cat > "$HOME/.local/bin/session-action" <<'SH'
#!/bin/sh
echo "$1 sid=$(ps -o sid= -p $$ | tr -d ' ')" >> /tmp/actions
SH
chmod +x "$HOME/.local/bin/session-action"

export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
pointer() { /bin/swaydesk-vpointer --size 1280x720 "$@"; }
shot() { grim "/out/session-menu-$impl-$1.png" 2>/dev/null; }
pidfile=$XDG_RUNTIME_DIR/session-menu.pid
menu() { env G_DEBUG=fatal-criticals SWAY_SESSION_MENU_IMPL=$impl "$HOME/.local/bin/session-menu" >> /tmp/menu.log 2>&1 & }
up() {  # espera o menu abrir e desenhar; devolve o PID
  for _ in $(seq 50); do [ -e "$pidfile" ] && break; sleep 0.1; done
  sleep 1
  cat "$pidfile" 2>/dev/null
}
gone() {  # o processo saiu em até 2 s
  for _ in $(seq 20); do kill -0 "$1" 2>/dev/null || return 0; sleep 0.1; done
  return 1
}

menu
pid=$(up)
if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
  fail "o menu não subiu"; cat /tmp/menu.log; exit 1
fi
exe=$(readlink /proc/$pid/exe)
case "$impl:$exe" in rust:*swaydesk-session-menu|python:*python3*) ok "sobe ($exe, comm $(cat /proc/$pid/comm))" ;; *) fail "binário errado: $exe" ;; esac
shot 1-aberto
pointer move 60 60 sleep 100 click sleep 300
gone "$pid" && ok "clicar fora fecha" || { fail "clicar fora não fechou"; kill "$pid"; }

menu
pid=$(up)
# Um clique no cartão (no cabeçalho, fora dos botões) não fecha
pointer move 640 290 sleep 100 click sleep 500
kill -0 "$pid" 2>/dev/null && ok "clicar no cartão não fecha" || fail "clicar no cartão fechou"
"$HOME/.local/bin/session-menu" & wait $!
gone "$pid" && ok "abrir de novo fecha" || { fail "abrir de novo não fechou"; kill "$pid"; }
[ -e "$pidfile" ] && fail "o arquivo de PID ficou" || ok "apaga o arquivo de PID"

menu
pid=$(up)
# O terceiro botão (Suspender) fica no centro da tela; a fileira, abaixo do cabeçalho
pointer move 640 370 sleep 100 click sleep 300
gone "$pid" && ok "o botão fecha o menu" || { fail "o botão não fechou o menu"; kill "$pid"; }
for _ in $(seq 20); do [ -s /tmp/actions ] && break; sleep 0.1; done
action=$(cat /tmp/actions 2>/dev/null)
case $action in
  "suspend sid="*) [ "${action#*sid=}" != "$(ps -o sid= -p $$ | tr -d ' ')" ] && ok "roda a ação ($action)" || fail "a ação ficou na sessão do menu" ;;
  *) fail "a ação não rodou ou veio errada: '$action'" ;;
esac
grep -q "CRITICAL\|Traceback" /tmp/menu.log && { fail "avisos no log"; cat /tmp/menu.log; }
exit $failed
