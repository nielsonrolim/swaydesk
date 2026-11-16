#!/bin/bash
# O swaydesk-click-catcher num Sway headless com duas telas. Roda dentro do contêiner swaydesk-smoke:debian13, com o
# repositório em /repo e o binário em /bin:
#
#   docker run --rm -u $(id -u):$(id -g) -v $PWD:/repo:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-click-catcher:/bin/swaydesk-click-catcher:ro \
#     swaydesk-smoke:debian13 /repo/rust/scripts/headless-click-catcher.sh
#
# Sem ponteiro no headless, o clique fica para a VM; aqui, a vida do catcher: com um processo que já terminou, sai na
# hora; com um vivo, uma camada mapeada por tela; quando o processo termina, sai logo. Sai com 1 se algum caso falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=2 LANG=C.UTF-8 GTK_A11Y=none
printf 'output HEADLESS-1 resolution 1280x720\noutput HEADLESS-2 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1
failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
catcher=/bin/swaydesk-click-catcher

# Um processo que já terminou: sai na hora, com 0
sleep 0 & gone=$!; wait "$gone"
if timeout 5 "$catcher" --pid "$gone"; then ok "processo já terminado: sai com 0"; else fail "processo já terminado: não saiu com 0"; fi
# Uso errado: 2
timeout 5 "$catcher" > /dev/null 2>&1; [ $? = 2 ] && ok "sem --pid: sai com 2" || fail "sem --pid não deu 2"

# Um processo vivo: uma camada por tela, mapeada; quando ele termina, o catcher sai
sleep 60 & target=$!
SWAYDESK_UI_DEBUG=1 "$catcher" --pid "$target" > /tmp/catcher.log 2>&1 & pid=$!
# As duas camadas avisam do mapa um segundo depois de abrir (SWAYDESK_UI_DEBUG): até 5 s, numa máquina ocupada
for _ in $(seq 50); do [ "$(grep -c 'catcher: mapped=' /tmp/catcher.log)" -ge 2 ] && break; sleep 0.1; done
grep -q 'catchers: 2 for 2 monitors' /tmp/catcher.log && ok "duas camadas para as duas telas" || { fail "camadas"; cat /tmp/catcher.log; }
[ "$(grep -c 'catcher: mapped=true' /tmp/catcher.log)" = 2 ] && ok "as duas mapeadas" || { fail "não mapeadas"; cat /tmp/catcher.log; }
kill -0 "$pid" 2>/dev/null && ok "fica enquanto o processo vive" || fail "saiu antes do processo"
kill "$target"
for _ in $(seq 20); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
if kill -0 "$pid" 2>/dev/null; then fail "não saiu 2 s depois do processo"; kill "$pid"; else ok "sai quando o processo termina"; fi
grep -q "CRITICAL\|panicked" /tmp/catcher.log && { fail "avisos no log"; cat /tmp/catcher.log; }
exit $failed
