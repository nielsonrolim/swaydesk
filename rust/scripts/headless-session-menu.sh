#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o menu da sessão (Rust e Python) num Sway headless, com o ponteiro virtual (swaydesk-vpointer).
# Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório em /repo, os binários em /bin e uma
# pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-session-menu:/bin/swaydesk-session-menu:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-session-menu.sh [rust|python]
#
# O session-action é trocado por um falso, que grava a ação. Os casos: o menu sobe com a versão pedida;
# clicar fora fecha; abrir de novo fecha; clicar num botão fecha o menu e roda a ação, numa sessão
# própria; um CRITICAL do GTK derruba o menu (G_DEBUG). A confirmação (--confirm, a do sway-sessionmgr):
# Cancelar e clicar fora não agem; "Reiniciar agora" age; ela abre no lugar do menu e de outra confirmação,
# e o menu aberto sobre ela a cancela. O teclado (letras, setas, Esc) e a contagem de 60 s ficam para a VM:
# o headless não tem teclado virtual. Sai com 1 se algum caso falhar.
set -u
impl=${1:-rust}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
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
menu() { env G_DEBUG=fatal-criticals SWAY_SESSION_MENU_IMPL=$impl "$HOME/.local/bin/session-menu" "$@" >> /tmp/menu.log 2>&1 & }
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

# A confirmação: o cartão no centro, com Cancelar (à esquerda) e Reiniciar agora (à direita) na fileira
cancel_x=560 accept_x=730 row_y=371
no_action() { sleep 0.5; [ -s /tmp/actions ] && fail "$1: rodou '$(cat /tmp/actions)'" || ok "$1: não age"; }
: > /tmp/actions
menu --confirm reboot
pid=$(up)
[ -n "$pid" ] && grep -q -- "--confirm" /proc/$pid/cmdline && ok "a confirmação sobe" || fail "a confirmação não subiu"
shot 2-confirmar
pointer move $cancel_x $row_y sleep 100 click sleep 300
gone "$pid" && ok "Cancelar fecha" || { fail "Cancelar não fechou"; kill "$pid"; }
no_action "Cancelar"

menu --confirm reboot
pid=$(up)
pointer move 60 60 sleep 100 click sleep 300
gone "$pid" && ok "clicar fora fecha a confirmação" || { fail "clicar fora não fechou a confirmação"; kill "$pid"; }
no_action "clicar fora"

# Outra confirmação, o menu e de novo a confirmação: cada um abre no lugar do anterior, sem agir
menu --confirm reboot
pid=$(up)
menu --confirm poweroff
pid2=$(for _ in $(seq 50); do p=$(cat "$pidfile" 2>/dev/null); [ -n "$p" ] && [ "$p" != "$pid" ] && kill -0 "$p" 2>/dev/null && { echo "$p"; break; }; sleep 0.1; done)
gone "$pid" && [ -n "$pid2" ] && ok "outra confirmação abre no lugar" || fail "outra confirmação não substituiu a aberta"
menu
pid3=$(for _ in $(seq 50); do p=$(cat "$pidfile" 2>/dev/null); [ -n "$p" ] && [ "$p" != "$pid2" ] && kill -0 "$p" 2>/dev/null && { echo "$p"; break; }; sleep 0.1; done)
gone "$pid2" && [ -n "$pid3" ] && ! grep -q -- "--confirm" /proc/$pid3/cmdline && ok "o menu cancela a confirmação e abre" || fail "o menu não abriu no lugar da confirmação"
menu --confirm reboot
pid4=$(for _ in $(seq 50); do p=$(cat "$pidfile" 2>/dev/null); [ -n "$p" ] && [ "$p" != "$pid3" ] && kill -0 "$p" 2>/dev/null && { echo "$p"; break; }; sleep 0.1; done)
gone "$pid3" && [ -n "$pid4" ] && ok "a confirmação abre no lugar do menu" || fail "a confirmação não abriu no lugar do menu"
sleep 1
no_action "trocas"
pointer move $accept_x $row_y sleep 100 click sleep 300
gone "$pid4" && ok "Reiniciar agora fecha" || { fail "Reiniciar agora não fechou"; kill "$pid4"; }
for _ in $(seq 20); do [ -s /tmp/actions ] && break; sleep 0.1; done
case $(cat /tmp/actions 2>/dev/null) in
  "reboot sid="*) ok "Reiniciar agora reinicia" ;;
  *) fail "Reiniciar agora: '$(cat /tmp/actions 2>/dev/null)'" ;;
esac
grep -q -- "--confirm takes" /tmp/menu.log && fail "pedido inválido no log"
"$HOME/.local/bin/session-menu" --confirm suspend 2>/dev/null; [ $? -eq 2 ] && ok "--confirm suspend sai com 2" || fail "--confirm suspend não saiu com 2"

grep -q "CRITICAL\|Traceback" /tmp/menu.log && { fail "avisos no log"; cat /tmp/menu.log; }
exit $failed
