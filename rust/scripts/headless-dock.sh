#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa a dock (Rust e Python) num Sway headless, com o ponteiro virtual (swaydesk-vpointer). Roda dentro
# do contêiner swaydesk-smoke:debian13, com o repositório em /repo, os binários em /bin e uma pasta
# gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) --group-add $(getent group render | cut -d: -f3) --device /dev/dri \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-dock:/bin/swaydesk-dock:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     -v /usr/share/icons:/usr/share/icons:ro -v /tmp/out:/out swaydesk-smoke:debian13 \
#     /repo/rust/scripts/headless-dock.sh [rust|python]
#
# Um app de teste (uma janela GTK 4 em Python, app_id "dock-test-app") fica fixado. Os casos: a dock
# sobe e publica os fixados; clicar no item abre o app numa sessão própria; com uma janela, clicar foca;
# o botão direito abre o menu, e um clique fora ou o Esc (com o wtype) o fecha; a janela cobrindo a dock
# a esconde; o ponteiro na borda a traz de volta; o "sway-dock pin" chega; o reload refaz a dock e solta
# a anterior. Sai com 1 se algum caso falhar.
set -u
. /repo/rust/scripts/lib-wait.sh
impl=${1:-rust}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" "$HOME/.local/share/applications" \
  "$HOME/.local/share/sway-dock" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
cat > /tmp/dock-test-app.py <<'PY'
import os
with open("/tmp/dock-test-app.env", "w") as f:  # o ambiente que a dock passou
    f.write("\n".join(f"{k}={v}" for k, v in os.environ.items()))
import gi
gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk
GLib.set_prgname("dock-test-app")
Gtk.init()
window = Gtk.Window(title="Dock test")
window.present()
loop = GLib.MainLoop()
window.connect("close-request", lambda *_a: loop.quit())
loop.run()
PY
cat > "$HOME/.local/share/applications/dock-test-app.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Dock test app
Exec=python3 /tmp/dock-test-app.py
Icon=utilities-terminal
DESKTOP
echo dock-test-app > "$HOME/.local/share/sway-dock/pinned"
# O botão da grade grava o ambiente que recebeu, em vez de abrir a grade
# Uma chave inválida no config.toml do desktop: o aviso sai uma vez, mesmo com a dock lendo a config de novo (a
# paleta a cada reload)
printf 'cursor_theme = "Breeze Light"\n' > "$HOME/.config/swaydesk/config.toml"
mkdir -p "$HOME/.config/sway-dock"
printf 'launcher = "sh -c \x27env > /tmp/launcher.env\x27"\n' > "$HOME/.config/sway-dock/config.toml"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
# Um teclado que fica: um wtype novo manda a tecla antes de o cliente ligar o wl_keyboard
kbd=
command -v wtype >/dev/null && { wtype -s 3600000 -k F24 & kbd=$!; }
trap 'kill $keeper $kbd 2>/dev/null' EXIT
sleep 0.5

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
dump() {  # o estado da dock (SIGUSR2) no log dela
  : > /tmp/dock.log.mark; kill -USR2 "$(cat "$XDG_RUNTIME_DIR/sway-dock.pid")"; sleep 0.3
  tail -n 3 /tmp/dock.log
}
pointer() { /bin/swaydesk-vpointer --size 1280x720 "$@"; }
shot() { grim "/out/$impl-$1.png" 2>/dev/null; }

# O escopo do app (só a dock em Rust o pede): a dock sobe como no login normal, pelo laço do config do Sway, sem o
# INVOCATION_ID de um serviço, com o gerenciador do systemd do usuário no ar (o systemd/private do
# XDG_RUNTIME_DIR). O systemd é um falso num barramento só deste teste: anota cada StartTransientUnit (o nome, a
# slice e os PIDs) e responde com o job e o JobRemoved dele, como o de verdade
if [ "$impl" = rust ]; then
  eval "$(dbus-daemon --session --fork --print-address=1 --print-pid=3 3>/tmp/dbus.pid | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"
  mkdir -p "$XDG_RUNTIME_DIR/systemd" && : > "$XDG_RUNTIME_DIR/systemd/private"
  cat > /tmp/fake-systemd.py <<'PY'
from gi.repository import Gio, GLib
XML = """<node><interface name="org.freedesktop.systemd1.Manager">
<method name="Subscribe"/><method name="Unsubscribe"/>
<method name="StartTransientUnit"><arg type="s" direction="in"/><arg type="s" direction="in"/>
<arg type="a(sv)" direction="in"/><arg type="a(sa(sv))" direction="in"/><arg type="o" direction="out"/></method>
<signal name="JobRemoved"><arg type="u"/><arg type="o"/><arg type="s"/><arg type="s"/></signal>
</interface></node>"""
jobs = [0]
def call(conn, sender, path, iface, method, params, invocation):
    if method != "StartTransientUnit":
        return invocation.return_value(None)
    name, _, props, _ = params.unpack()
    props = dict(props)
    with open("/tmp/scope-calls", "a") as f:
        f.write(f"{name} {props.get('Slice')} {props.get('PIDs')}\n")
    jobs[0] += 1
    job = f"/org/freedesktop/systemd1/job/{jobs[0]}"
    invocation.return_value(GLib.Variant("(o)", (job,)))
    conn.emit_signal(None, "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager", "JobRemoved",
                     GLib.Variant("(uoss)", (jobs[0], job, name, "done")))
def acquired(conn, name):
    conn.register_object("/org/freedesktop/systemd1", Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0], call)
    open("/tmp/fake-systemd.ready", "w").close()
Gio.bus_own_name(Gio.BusType.SESSION, "org.freedesktop.systemd1", Gio.BusNameOwnerFlags.NONE, acquired, None, None)
GLib.MainLoop().run()
PY
  python3 /tmp/fake-systemd.py & fake_systemd=$!
  trap 'kill $keeper $kbd $fake_systemd "$(cat /tmp/dbus.pid 2>/dev/null)" 2>/dev/null' EXIT
  for _ in $(seq $ticks); do [ -e /tmp/fake-systemd.ready ] && break; sleep 0.1; done
fi

# Um CRITICAL do GTK derruba a dock: o teste falha em vez de passar com o aviso no log
env G_DEBUG=fatal-criticals SWAY_DOCK_IMPL=$impl "$HOME/.local/bin/sway-dock" > /tmp/dock.log 2>&1 & dock=$!
for _ in $(seq $ticks); do [ -e "$XDG_RUNTIME_DIR/sway-dock.pid" ] && break; sleep 0.1; done
sleep 1.5
pid=$(cat "$XDG_RUNTIME_DIR/sway-dock.pid" 2>/dev/null)
if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
  fail "a dock não subiu"; cat /tmp/dock.log; exit 1
fi
exe=$(readlink /proc/$pid/exe)
case "$impl:$exe" in rust:*swaydesk-dock|python:*python3*) ok "sobe ($exe, comm $(cat /proc/$pid/comm))" ;; *) fail "binário errado: $exe" ;; esac
grep -qx dock-test-app "$XDG_RUNTIME_DIR/sway-dock.pinned-ids" && ok "publica os fixados" || fail "sem a lista dos fixados"
shot 1-visivel
dump | grep -q "visible=True\|visible=true" && ok "visível sem janelas" || fail "não aparece sem janelas"

# O item fixado é o primeiro: o centro da dock menos meio lugar (dois lugares de 56 px)
item_x=612 item_y=688
pointer move $item_x $item_y sleep 200 click sleep 200
for _ in $(seq $ticks); do swaymsg -t get_tree | grep -q '"app_id": "dock-test-app"' && break; sleep 0.1; done
app=$(pgrep -f dock-test-app.py | head -1)
if [ -n "$app" ]; then
  ok "o clique abre o app"
  # Pelo laço do Sway (sem o INVOCATION_ID), com o systemd do usuário: o app pede o seu escopo na app.slice
  if [ "$impl" = rust ]; then
    if grep -q '^app-swaydesk-dock\\x2dtest\\x2dapp-[0-9]*\.scope app\.slice \[[0-9]' /tmp/scope-calls 2>/dev/null; then
      ok "fora de um serviço, o app vai para o seu escopo ($(head -1 /tmp/scope-calls))"
    else
      fail "sem o escopo do app pelo laço do Sway: $(cat /tmp/scope-calls 2>/dev/null)"; tail -5 /tmp/dock.log
    fi
  fi
  # O "Encerrar" do popup do sistema trata como auxiliar da dock o filho dela na mesma sessão: o app
  # precisa de outra sessão ou de outro pai (o GLib solta o app com um fork duplo)
  ppid=$(ps -o ppid= -p "$app" | tr -d ' ')
  if [ "$ppid" != "$pid" ] || [ "$(ps -o sid= -p "$app")" != "$(ps -o sid= -p "$pid")" ]; then
    ok "o app não fica como auxiliar da dock (pai $ppid, sessão $(ps -o sid= -p "$app" | tr -d ' '))"
  else
    fail "o app é filho da dock na mesma sessão"
  fi
else
  fail "o clique não abriu o app"
fi
# O cairo que a dock escolheu para si não passa ao app (os apps em GTK 4 desenhariam pela CPU)
if [ -e /tmp/dock-test-app.env ] && ! grep -q '^GSK_RENDERER=' /tmp/dock-test-app.env; then
  ok "o app não herda o GSK_RENDERER da dock"
else
  fail "o app herdou o GSK_RENDERER: $(grep '^GSK_RENDERER=' /tmp/dock-test-app.env 2>/dev/null)"
fi
sleep 1
dump | grep -q "overlapped=True\|overlapped=true" && ok "a janela cobre a dock" || fail "não viu a janela por cima"
sleep 1
shot 2-escondida
dump | grep -q "visible=False\|visible=false" && ok "esconde com a janela por cima" || fail "não escondeu"

pointer move 640 719 sleep 300
sleep 0.5
shot 3-borda
dump | grep -q "visible=True\|visible=true" && ok "a borda traz a dock" || fail "a borda não trouxe a dock"

# As dicas, acima do item (a do GTK 4 abria embaixo e saía cortada): só a captura, para comparar
# Antes, a dock termina de esconder: visível, ela cobre a faixa da borda, e a margem transparente embaixo
# dela não recebe o ponteiro (nas duas versões), então a borda não a seguraria
sleep 1
# Cada swaydesk-vpointer é um dispositivo novo: ao sair, o ponteiro sai da dock, que esconde. A captura
# vai durante o sleep dele, e a borda vem antes, na mesma chamada (saindo de outro ponto: o cursor já
# parado na borda não gera a entrada)
# A primeira passada depois do caso da borda não chega à dock no headless (nas duas versões, em qualquer
# ordem): uma passada antes, descartada
pointer move 640 600 sleep 100 move 640 719 sleep 500 move $item_x $item_y sleep 300 move 640 500 sleep 100
sleep 1
pointer move 640 600 sleep 100 move 640 719 sleep 500 move $((item_x + 56)) $item_y sleep 2000 & sleep 1.8; shot 6-dica-grade; wait $!
pointer move 640 600 sleep 100 move 640 719 sleep 500 move $item_x $item_y sleep 2000 & sleep 1.8; shot 5-dica; wait $!
# O botão da grade, à direita do item (com a dock à vista)
pointer move 640 600 sleep 100 move 640 719 sleep 500 move $((item_x + 56)) $item_y sleep 300 click sleep 500
if [ -e /tmp/launcher.env ] && ! grep -q '^GSK_RENDERER=' /tmp/launcher.env; then
  ok "o botão da grade roda o lançador, sem o GSK_RENDERER"
else
  fail "o lançador não rodou ou herdou o GSK_RENDERER"
fi
pointer move 640 719 sleep 300
sleep 0.5
pointer move $item_x $item_y sleep 300 click right sleep 500
shot 4-menu
dump | grep -q "menu=on screen" && ok "o botão direito abre o menu" || fail "o menu não abriu"
pointer move 300 200 sleep 200 click sleep 500
dump | grep -q "menu=no" && ok "o clique fora fecha o menu" || fail "o menu não fechou"
# O Sway não dá o teclado aos popups: a dock pega o teclado enquanto o menu está aberto
if [ -n "$kbd" ]; then
  pointer move 640 719 sleep 300
  sleep 0.5
  pointer move $item_x $item_y sleep 300 click right sleep 500
  wtype -k Escape; sleep 0.5
  dump | grep -q "menu=no" && ok "o Esc fecha o menu" || fail "o Esc não fechou o menu"
  pointer move 300 200 sleep 200
  # Um menu que não chega à tela é fechado na hora: um que abre normalmente nunca pode ser fechado por
  # engano. Várias aberturas seguidas, metade logo depois de trocar de workspace; cada uma fica na tela
  # e com o teclado (o Esc a fecha)
  opened=0 kept=0 rounds=8
  for r in $(seq $rounds); do
    pointer move 640 719 sleep 300
    sleep 0.3
    [ $((r % 2)) = 0 ] && swaymsg -q workspace 2 && swaymsg -q workspace 1
    pointer move $item_x $item_y sleep 300 click right sleep 500
    dump | grep -q "menu=on screen" && opened=$((opened + 1))
    wtype -k Escape; sleep 0.5
    dump | grep -q "menu=no" && kept=$((kept + 1))
    pointer move 300 200 sleep 200
  done
  [ $opened = $rounds ] && ! grep -q "menu did not open" /tmp/dock.log \
    && ok "o menu abre e fica na tela $rounds vezes seguidas, também depois de trocar de workspace" \
    || fail "o menu ficou na tela $opened de $rounds vezes"
  [ $kept = $rounds ] && ok "o Esc fecha o menu em todas" || fail "o Esc fechou o menu $kept de $rounds vezes"
else
  printf 'pulado  o Esc fecha o menu (sem o wtype)\n'
fi

"$HOME/.local/bin/sway-dock" pin org.example.Other && sleep 0.5
grep -qx org.example.Other "$HOME/.local/share/sway-dock/pinned" && ok "o pin chega" || fail "o pin não chegou"
"$HOME/.local/bin/sway-dock" unpin org.example.Other && sleep 0.5
grep -qx org.example.Other "$HOME/.local/share/sway-dock/pinned" && fail "o unpin não chegou" || ok "o unpin chega"

if [ "$impl" = rust ]; then
  "$HOME/.local/bin/sway-dock" reload; sleep 1
  "$HOME/.local/bin/sway-dock" reload; sleep 1
  dump | grep -q "old docks still alive: 0 of" && ok "o reload solta as docks anteriores" || fail "uma dock anterior continua viva"
  n=$(grep -c "config: invalid theme name in cursor_theme: Breeze Light" /tmp/dock.log)
  [ "$n" = 1 ] && ok "o aviso da config sai uma vez, com os reloads" || fail "o aviso da config saiu $n vezes (esperado 1)"
fi
grep -E "^(Rss|Pss|Private_Dirty):" /proc/$pid/smaps_rollup | tr -s ' ' | paste -sd ' ' | sed 's/^/memória: /'
kill "$pid"; sleep 0.5
kill -0 "$pid" 2>/dev/null && fail "não saiu com o TERM" || ok "sai com o TERM"
[ -e "$XDG_RUNTIME_DIR/sway-dock.pid" ] && fail "o arquivo do PID ficou" || ok "apaga o arquivo do PID"
grep -v "^$" /tmp/dock.log | grep -iv "old docks\|^sway-dock: dock 0" | head -20 | sed 's/^/log: /'
exit $failed
