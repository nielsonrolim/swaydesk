#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# As entradas só da busca da grade de apps (Rust ou Python), num Sway headless: um .desktop NoDisplay com
# X-Swaydesk-Searchable=true (como os painéis da Central) não aparece na grade nem nos favoritos, e a busca o acha.
# Roda dentro do contêiner swaydesk-smoke:debian13, com o repositório em /repo, o binário da grade em /bin e uma
# pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-drawer:/bin/swaydesk-drawer:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-drawer-search-only.sh [rust|python]
#
# Cada app abre com um "touch /tmp/launched-<nome>", e a conferência é por qual arquivo apareceu. Os casos:
# 1. Sem busca, um clique no primeiro lugar da grade (o swaydesk-vpointer) abre o app visível: o só da busca, que
#    vem antes dele no nome e está na lista de favoritos (que ficariam em cima), não está lá.
# 2. A palavra-chave do só da busca o acha, e o Enter o abre.
# 3. Um NoDisplay sem a chave, e um com a chave mas sem o programa do TryExec, não aparecem nem na busca.
# 4. Entradas sem Exec não derrubam a grade (o snap prompting-client do Ubuntu 26.04: NoDisplay, DBusActivatable,
#    sem Exec; o g_app_info_get_executable dá NULL, que o gio-rs lia como texto): o caso 1 já não abriria a
#    grade. Nem elas, nem uma com Exec= vazio, nem uma Hidden aparecem na busca, e a grade segue no ar.
# Sai com 1 se algum caso falhar.
set -u
. /repo/rust/scripts/lib-wait.sh
impl=${1:-rust}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
apps=$HOME/.local/share/applications
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" "$apps" "$HOME/.local/share/sway-drawer" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do cp "$f" "$HOME/.local/bin/"; done
[ -x /bin/swaydesk-vpointer ] || { echo "FALHOU  falta o /bin/swaydesk-vpointer"; exit 1; }

# entry <id> <nome> <palavra-chave> <linhas extras>
entry() {
  printf '[Desktop Entry]\nType=Application\nName=%s\nKeywords=%s;\nExec=sh -c "touch /tmp/launched-%s"\nIcon=utilities-terminal\n%b' \
    "$2" "$3" "$1" "$4" > "$apps/$1.desktop"
}
# Os nomes começam com "Aaa": vêm antes dos apps da imagem, e o só da busca antes do visível
entry aaa-painel "Aaa Painel" zzpainel 'NoDisplay=true\nX-Swaydesk-Searchable=true\nTryExec=sh\n'
entry aab-visivel "Aab Visivel" zzvisivel ''
entry aaa-oculto "Aaa Oculto" zzoculto 'NoDisplay=true\n'
entry aaa-semprog "Aaa Sem Programa" zzsemprog 'NoDisplay=true\nX-Swaydesk-Searchable=true\nTryExec=/nonexistent/tool\n'
# Sem Exec (cada uma com a palavra-chave zzsemexec): o caso do snap, a mesma com a chave, um Exec= vazio, uma
# Hidden com a chave, e um app visível D-Bus sem Exec (o caminho dos apps visíveis)
noexec() {
  printf '[Desktop Entry]\nType=Application\nName=%s\nKeywords=zzsemexec;\nIcon=utilities-terminal\n%b' "$2" "$3" > "$apps/$1.desktop"
}
noexec aaa-snap "Aaa Snap" 'NoDisplay=true\nDBusActivatable=true\n'
noexec aaa-snapbusca "Aaa Snap Busca" 'NoDisplay=true\nDBusActivatable=true\nX-Swaydesk-Searchable=true\n'
noexec aaa-execvazio "Aaa Exec Vazio" 'NoDisplay=true\nX-Swaydesk-Searchable=true\nExec=\n'
noexec aaa-apagada "Aaa Apagada" 'Hidden=true\nX-Swaydesk-Searchable=true\nExec=sh -c "touch /tmp/launched-aaa-apagada"\n'
noexec aac-dbus "Aac Dbus" 'DBusActivatable=true\n'
# O só da busca nos favoritos (um fixado de antes, ou feito à mão): mesmo assim não aparece
printf 'aaa-painel\n' > "$HOME/.local/share/sway-drawer/pinned"

export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
# Sem um teclado vivo, cada wtype cria o seu, e o primeiro caractere se perde
wtype -s 3600000 -k F24 & keeper=$!
# E um ponteiro vivo, para o clique não esperar o Sway ver um dispositivo novo
/bin/swaydesk-vpointer sleep 3600000 & pointer_keeper=$!
trap 'kill $keeper $pointer_keeper 2>/dev/null' EXIT
sleep 0.5

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
shown() { [ -e "$XDG_RUNTIME_DIR/sway-drawer.shown" ]; }
hidden() { ! shown; }
close_grid() {
  shown || return 0
  # Com texto na busca, o primeiro Esc só a limpa; o segundo esconde a grade
  wtype -k Escape; sleep 0.3; shown && wtype -k Escape
  wait_for hidden || fail "a grade não fechou"
  sleep 0.5
}
open_grid() {
  close_grid
  rm -f /tmp/launched-*
  SWAY_DRAWER_IMPL=$impl "$HOME/.local/bin/sway-drawer"
  for _ in $(seq $ticks); do shown && break; sleep 0.1; done
  sleep 1
}
any_launched() { set -- /tmp/launched-*; [ -e "$1" ]; }
which_launched() { set -- /tmp/launched-*; [ -e "$1" ] && printf '%s' "${1#/tmp/launched-}"; }
launched() { wait_for any_launched; which_launched; }  # o que abriu, esperando até o prazo do lib-wait
# Nada deve abrir: não há condição para esperar, então uma pausa e a conferência
nothing_launched() { sleep 3; which_launched; }
# Abre o primeiro resultado com um clique no primeiro lugar: o teclado não serve aos dois (no Python, o Enter não
# chega à busca no headless, como no headless-drawer-qalc, e a seta não leva o cursor da grade)
FIRST_CELL="335 195"
open_first_result() { /bin/swaydesk-vpointer --size 1280x720 move $FIRST_CELL click sleep 200; }

SWAY_DRAWER_IMPL=$impl "$HOME/.local/bin/sway-drawer" start > /tmp/drawer.log 2>&1 &
sleep 2

# 1. Sem busca: o primeiro lugar da grade é o visível. A grade de 1280x720 põe o primeiro ícone em FIRST_CELL, e
# uma linha de favoritos o empurraria para baixo, com o só da busca no lugar
open_grid
shown && ok "a grade abre" || { fail "a grade não abriu"; cat /tmp/drawer.log; exit 1; }
grim "/out/$impl-search-only-1-grade.png" 2>/dev/null
open_first_result
got=$(launched)
[ "$got" = aab-visivel ] && ok "sem busca, o primeiro item é o app visível" \
  || fail "sem busca, o primeiro item abriu '$got' (o só da busca na grade ou nos favoritos?)"

# 2. A busca acha o só da busca
open_grid
wtype zzpainel
sleep 1
grim "/out/$impl-search-only-2-busca.png" 2>/dev/null
open_first_result
got=$(launched)
[ "$got" = aaa-painel ] && ok "a busca acha a entrada só da busca e a abre" || fail "a busca por zzpainel abriu '$got'"

# 3. Sem a chave, ou sem o programa: nem na busca
for case in zzoculto:aaa-oculto zzsemprog:aaa-semprog; do
  word=${case%%:*} id=${case#*:}
  open_grid
  wtype "$word"
  sleep 1
  open_first_result
  got=$(nothing_launched)
  [ -z "$got" ] && ok "a busca por $word não acha o $id" || fail "a busca por $word abriu '$got'"
done

# 4. As entradas sem Exec: a grade segue no ar, e a busca não abre nenhuma das escondidas
open_grid
shown && ok "a grade abre com entradas sem Exec" || fail "a grade não abriu com entradas sem Exec"
wtype zzsemexec
sleep 1
grim "/out/$impl-search-only-4-sem-exec.png" 2>/dev/null
open_first_result
got=$(nothing_launched)
case $got in aaa-*) fail "a busca por zzsemexec abriu a escondida '$got'" ;; *) ok "a busca por zzsemexec não abre nenhuma escondida" ;; esac
pgrep -u "$(id -u)" -f 'sway-drawer|swaydesk-drawer' > /dev/null && ok "a grade não caiu" || fail "a grade caiu"
close_grid
[ "$failed" = 0 ] || { echo "--- registro da grade"; cat /tmp/drawer.log; }
exit "$failed"
