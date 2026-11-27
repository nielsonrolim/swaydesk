#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele (o
# tests/run-all-tests.sh o roda com o UID 64000, fora do pgrep -u da sua sessão).
# Testa a Central de Configurações (swaydesk-settings, 0.2.0) num Sway headless. Roda dentro do contêiner
# swaydesk-smoke:debian13, com o repositório em /repo, o binário em /bin e uma pasta gravável em /out:
#
#   docker run --rm --memory 10g --memory-swap 10g -u 64000:64000 -e HOME=/tmp -v passwd-64000:/etc/passwd:ro \
#     -v $PWD/rust/target/release/swaydesk-vpointer:/bin/swaydesk-vpointer:ro \
#     -v $PWD:/repo:ro -v $PWD/rust/target/release/swaydesk-settings:/bin/swaydesk-settings:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-settings.sh
#
# (o passwd-64000: o /etc/passwd da imagem com uma linha para o UID 64000, que o dbus-daemon pede; o
# tests/run-all-tests.sh monta o seu)
#
# Os casos: um painel desconhecido sai com 2 antes do GTK (sem display nem barramento); a Central abre no
# primeiro painel, com o app_id e o título certos; uma segunda chamada com --panel troca o painel da mesma
# janela (o GtkApplication único) e volta; encaminhado pelo D-Bus à janela aberta (o tratador dela, que o binário
# não alcança com um argumento errado), um painel desconhecido volta 2 e não mexe nela;
# todo painel próprio abre pelo id e pelo apelido; o painel Aparência (a página do app Tema) aplica um tema; o painel Telas (a página do app Telas) respeita a trava da prévia de outro processo e segura a sua; um painel de fora sem o programa instalado, e o Aparência ou o
# Telas sem o pacote de que dependem, saem com 1 e não abrem janela; fechar a janela encerra o app. Sai com 1 se
# algum caso falhar.
set -u
. /repo/rust/scripts/lib-wait.sh
export HOME=${HOME:-/tmp} XDG_RUNTIME_DIR=/tmp/xdg-settings
mkdir -p -m 700 "$XDG_RUNTIME_DIR"
bin=/bin/swaydesk-settings
app_id=org.swaydesktop.Settings

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }

# 1. Antes de tudo, sem display e sem barramento: o --panel errado sai com 2 e diz qual
out=$(env -u WAYLAND_DISPLAY -u DISPLAY -u DBUS_SESSION_BUS_ADDRESS "$bin" --panel nope 2>&1); rc=$?
if [ "$rc" = 2 ] && printf '%s' "$out" | grep -q 'nope'; then ok "--panel nope sai com 2 sem display"; else fail "--panel nope: rc=$rc, saída: $out"; fi
out=$(env -u WAYLAND_DISPLAY -u DISPLAY -u DBUS_SESSION_BUS_ADDRESS "$bin" --frobnicate 2>&1); rc=$?
[ "$rc" = 2 ] && ok "um argumento desconhecido sai com 2" || fail "--frobnicate: rc=$rc, saída: $out"

# 1b. O relato do Sobre (--about), sem display: os arquivos do sistema lidos de verdade, e sem o nome da máquina
out=$(env -u WAYLAND_DISPLAY -u DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u SWAYSOCK "$bin" --about 2>&1); rc=$?
distro=$(. /etc/os-release && printf '%s' "$PRETTY_NAME")
[ "$rc" = 0 ] && ok "--about sai com 0" || fail "--about: rc=$rc, saída: $out"
printf '%s\n' "$out" | grep -qxF "Distribution: $distro" && ok "--about: a distro do os-release" || fail "--about sem 'Distribution: $distro': $out"
printf '%s\n' "$out" | grep -qxF "Kernel: $(uname -r)" && ok "--about: o kernel" || fail "--about sem o kernel $(uname -r): $out"
printf '%s\n' "$out" | grep -qE '^Memory: [0-9.]+ [KMGT]iB$' && ok "--about: a memória" || fail "--about sem a memória: $out"
printf '%s\n' "$out" | grep -qE '^CPU: .+' && ok "--about: o processador" || fail "--about sem o processador: $out"
printf '%s\n' "$out" | grep -qE '^Disk \(/\): [0-9.]+ [KMGT]?i?B used of [0-9.]+ [KMGT]iB$' && ok "--about: o disco" || fail "--about sem o disco: $out"
host=$(cat /proc/sys/kernel/hostname)
printf '%s\n' "$out" | grep -qwF -e "$host" -e Hostname && fail "--about tem o nome da máquina ($host): $out" || ok "--about sem o nome da máquina"

export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8 GTK_A11Y=none
printf 'output HEADLESS-1 resolution 1280x800\n' > /tmp/sway-settings.conf
# O app é único pelo D-Bus da sessão (um barramento só deste teste)
eval "$(dbus-daemon --session --fork --print-address=1 --print-pid=3 3>/tmp/dbus.pid | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"
sway -c /tmp/sway-settings.conf > /tmp/sway.log 2>&1 &
sway_pid=$!
trap 'kill $sway_pid 2>/dev/null; kill "$(cat /tmp/dbus.pid 2>/dev/null)" 2>/dev/null' EXIT
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK
SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock)
export G_DEBUG=fatal-criticals

# O painel Desktop lê os arquivos quando a página é montada (no caso 5, que abre todos): antes do app subir, o
# config.toml do usuário (com comentários), a camada do sistema e os programas que os interruptores ligam, falsos
# no ~/.local/bin (o paths::component e o find_helper os acham ali), que só anotam a chamada
config_toml=$HOME/.config/swaydesk/config.toml
mkdir -p "${config_toml%/*}" /tmp/etc "$HOME/.local/bin"
printf '# mine\n[enabled]\ndock = false  # no dock\n' > "$config_toml"
printf '[enabled]\nosd = false\n' > /tmp/etc/system.toml
export SWAYDESK_SYSTEM_CONFIG=/tmp/etc/system.toml
for program in sway-dock sway-drawer sway-switcher sway-osd-bar sway-nightlight bing-wallpaper sway-update-notifier \
  sway-waybar sway-desktop-config; do
  printf '#!/bin/sh\necho "%s $*" >> /tmp/calls\n' "$program" > "$HOME/.local/bin/$program"
  chmod +x "$HOME/.local/bin/$program"
done
: > /tmp/calls

# 1c. Com o Sway no ar, o relato tem a versão dele
sway_version=$(swaymsg -t get_version | python3 -c 'import json, sys; print(json.load(sys.stdin)["human_readable"])')
out=$("$bin" --about 2>&1)
printf '%s\n' "$out" | grep -qxF "Sway: $sway_version" && ok "--about: o Sway $sway_version" || fail "--about sem 'Sway: $sway_version': $out"

windows() { swaymsg -t get_tree | grep -c "\"app_id\": \"$app_id\""; }
title() { swaymsg -t get_tree | python3 -c '
import json, sys
def walk(n):
    if n.get("app_id") == sys.argv[1]:
        print(n.get("name") or "")
    for c in n.get("nodes", []) + n.get("floating_nodes", []):
        walk(c)
walk(json.load(sys.stdin))' "$app_id"; }
con_id() { swaymsg -t get_tree | python3 -c '
import json, sys
def walk(n):
    if n.get("app_id") == sys.argv[1]:
        print(n["id"])
    for c in n.get("nodes", []) + n.get("floating_nodes", []):
        walk(c)
walk(json.load(sys.stdin))' "$app_id"; }
# A chamada que uma segunda instância encaminha à primeira (org.gtk.Application.CommandLine), feita direto: o
# binário confere os argumentos antes de encaminhar, e só assim o tratador da janela aberta recebe um errado.
# Imprime o código que a primeira devolve
forward() { python3 - "$@" <<'PY'
import sys
from gi.repository import Gio, GLib
bus = Gio.bus_get_sync(Gio.BusType.SESSION)
args = [("swaydesk-settings\0").encode()] + [(a + "\0").encode() for a in sys.argv[1:]]
reply = bus.call_sync("org.swaydesktop.Settings", "/org/swaydesktop/Settings", "org.gtk.Application", "CommandLine",
                      GLib.Variant("(oaaya{sv})", ("/org/swaydesktop/Settings/CommandLine/test", args, {})),
                      GLib.VariantType("(i)"), Gio.DBusCallFlags.NONE, 10000, None)
print(reply.unpack()[0])
PY
}
wait_windows() { for _ in $(seq $ticks); do [ "$(windows)" = "$1" ] && return 0; sleep 0.1; done; return 1; }
wait_title() { for _ in $(seq $ticks); do [ "$(title)" = "$1" ] && return 0; sleep 0.1; done; return 1; }

# Um power-profiles-daemon falso (o busctl do PATH do app, que guarda o PATH com que abriu): responde 1,5 s depois,
# como um daemon lento, e anota cada set-property. O painel Energia não pode esperar por ele ao abrir
mkdir -p /tmp/fakebin
cat > /tmp/fakebin/busctl <<'BUSCTL'
#!/bin/sh
case "$*" in
  *set-property*) echo "busctl $*" >> /tmp/calls ;;
  # O ActiveProfile antes: o nome da interface (PowerProfiles) também casaria com o *Profiles*
  *ActiveProfile*) sleep 1.5; echo 's "balanced"' ;;
  *Profiles*) sleep 1.5; echo '{"type":"aa{sv}","data":[{"Profile":{"type":"s","data":"power-saver"}},{"Profile":{"type":"s","data":"balanced"}},{"Profile":{"type":"s","data":"performance"}}]}' ;;
esac
BUSCTL
chmod +x /tmp/fakebin/busctl
export PATH=/tmp/fakebin:$PATH

# O painel Apps padrão (o Gio lê as pastas quando o app sobe): apps falsos no XDG_DATA_DIRS, e uma camada do
# sistema como a das ISOs (o /etc/xdg/mimeapps.list, com o visualizador de imagens) no XDG_CONFIG_DIRS. Um foot
# falso no PATH, para o terminal ter um candidato além do automático
mkdir -p /tmp/share/applications /tmp/xdg-etc
fake_app() {
  printf '[Desktop Entry]\nType=Application\nName=%s\nExec=/bin/true %%U\nMimeType=%s\n' "$2" "$3" \
    > "/tmp/share/applications/$1.desktop"
}
fake_app fake-browser "Fake Browser" 'x-scheme-handler/http;x-scheme-handler/https;text/html;'
fake_app fake-viewer "Fake Viewer" 'image/png;image/jpeg;'
fake_app fake-photos "Fake Photos" 'image/png;'
# O Gio acha os apps de cada tipo pelo mimeinfo.cache (o update-desktop-database dos pacotes), não lendo os .desktop
printf '[MIME Cache]\nx-scheme-handler/http=fake-browser.desktop;\nx-scheme-handler/https=fake-browser.desktop;\ntext/html=fake-browser.desktop;\nimage/png=fake-viewer.desktop;fake-photos.desktop;\nimage/jpeg=fake-viewer.desktop;\n' \
  > /tmp/share/applications/mimeinfo.cache
printf '[Default Applications]\nimage/png=fake-viewer.desktop;\nimage/jpeg=fake-viewer.desktop;\n' > /tmp/xdg-etc/mimeapps.list
export XDG_DATA_DIRS=/tmp/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share} XDG_CONFIG_DIRS=/tmp/xdg-etc
# E o nemo e o thunar falsos: o automático é o nemo (o primeiro instalado), e o thunar é o outro candidato
for program in foot nemo thunar; do
  printf '#!/bin/sh\n' > "/tmp/fakebin/$program"
  chmod +x "/tmp/fakebin/$program"
done
mimeapps=$HOME/.config/mimeapps.list
# O Aparência e o Telas só aparecem com o programa do pacote de que dependem (o swaydesk-theme do
# swaydesk-theme-tool, o swaydesk-displays): falsos no PATH, a não ser que a imagem já os tenha
for program in swaydesk-theme swaydesk-displays; do
  command -v "$program" > /dev/null || { printf '#!/bin/sh\n' > "/tmp/fakebin/$program"; chmod +x "/tmp/fakebin/$program"; }
done
# O PATH sem esses programas, para os casos em que o pacote falta: uma pasta com um link para cada programa do
# PATH, menos eles (o tests/run-all-tests.sh monta os binários de verdade no /bin, e tirar a pasta do PATH não
# bastaria)
mkdir -p /tmp/barebin
IFS=: read -ra path_dirs <<< "$PATH"
for dir in "${path_dirs[@]}"; do
  for f in "$dir"/*; do
    name=${f##*/}
    case $name in swaydesk-theme|swaydesk-displays) continue ;; esac
    [ -x "$f" ] && [ ! -e "/tmp/barebin/$name" ] && ln -s "$f" "/tmp/barebin/$name"
  done
done
bare_path=/tmp/barebin
# O painel Aparência aplica com os modelos do sway-desktop-config (o binário não está ao lado de um share/), e a
# lista de ícones sem os do sistema (como no headless-theme-app)
mkdir -p /tmp/no-system-icons
export SWAY_DESKTOP_SHARE=/repo/home/.local/share/sway-desktop SWAY_SYSTEM_ICONS=/tmp/no-system-icons

# 2. Abre no primeiro painel
"$bin" >> /tmp/app.log 2>&1 &
app=$!
if wait_windows 1; then ok "abre uma janela com o app_id $app_id"; else fail "a janela não abriu"; cat /tmp/app.log; exit 1; fi
wait_title "Appearance — Settings" && ok "abre no painel Appearance" || fail "título: '$(title)'"
grim /out/settings-1-appearance.png 2>/dev/null
# A mesma janela até o fim (o con_id do Sway): uma primária que caísse e abrisse outra também teria o título certo
first=$(con_id)

# 3. Uma segunda chamada troca o painel da mesma janela e volta
timeout 10 "$bin" --panel displays >> /tmp/app.log 2>&1; rc=$?
[ "$rc" = 0 ] && ok "a segunda chamada volta com 0" || fail "a segunda chamada: rc=$rc"
wait_title "Displays — Settings" && ok "--panel displays troca o painel" || fail "título: '$(title)'"
[ "$(windows)" = 1 ] && [ "$(con_id)" = "$first" ] && ok "continua a mesma janela" || fail "$(windows) janelas, con_id $(con_id) (antes $first)"

# 4. Encaminhado à janela aberta: um painel desconhecido volta 2 e não mexe nela; um certo troca o painel
rc=$(forward --panel nope 2>> /tmp/app.log)
[ "$rc" = 2 ] && ok "--panel nope encaminhado volta com 2" || fail "--panel nope encaminhado: '$rc'"
rc=$(forward --frobnicate 2>> /tmp/app.log)
[ "$rc" = 2 ] && ok "um argumento desconhecido encaminhado volta com 2" || fail "--frobnicate encaminhado: '$rc'"
sleep 0.3
[ "$(title)" = "Displays — Settings" ] && [ "$(con_id)" = "$first" ] && ok "a janela não mudou" || fail "a janela mudou: '$(title)', con_id $(con_id)"
rc=$(forward --panel keyboard 2>> /tmp/app.log)
[ "$rc" = 0 ] && wait_title "Keyboard — Settings" && ok "--panel keyboard encaminhado troca o painel" || fail "--panel keyboard encaminhado: '$rc', '$(title)'"

# 5. Todo painel próprio, pelo id e por um apelido
for pair in "appearance:Appearance" "displays:Displays" "keyboard:Keyboard" "mouse:Mouse and touchpad" \
  "power:Power and lock" "desktop:Desktop" "default-apps:Default apps" "region:Region" "about:About" \
  "theme:Appearance" "touchpad:Mouse and touchpad"; do
  id=${pair%%:*} name=${pair#*:}
  start=$(date +%s%N)
  timeout 10 "$bin" --panel "$id" >> /tmp/app.log 2>&1
  wait_title "$name — Settings" && ok "--panel $id: $name" || fail "--panel $id: título '$(title)'"
  # O Energia abre sem esperar o daemon (1,5 s por pergunta): a leitura dos perfis é numa thread
  if [ "$id" = power ]; then
    took=$(( ($(date +%s%N) - start) / 1000000 ))
    [ "$took" -lt 1200 ] && ok "o Energia abre sem esperar o daemon dos perfis (${took} ms)" || fail "o Energia esperou o daemon: ${took} ms"
  fi
done
grim /out/settings-2-about.png 2>/dev/null
# O painel Sobre de verdade (a leitura é numa thread: um instante para os dados chegarem)
timeout 10 "$bin" --panel about >> /tmp/app.log 2>&1
wait_title "About — Settings" && ok "--panel about" || fail "--panel about: '$(title)'"
sleep 1
grim /out/settings-3-about-panel.png 2>/dev/null
# Uma página em branco também teria o título: os dados têm de estar na tela. Na área das linhas (abaixo do
# "Swaydesk", à direita da barra lateral), conta os pixels escuros do texto; o "…" de antes da leitura dá poucos.
# Medido em 25/11: 1715 com o painel, 191 com o fill() pulado. A área e o limite valem para a saída de 1280x800,
# o tamanho padrão da janela (900x620), a barra lateral de 220 px e o tema claro (texto escuro): quem mudar um
# deles mede de novo (o "pixels escuros" abaixo sai no log) e ajusta estes números
ABOUT_ROWS_Y="225 445" ABOUT_ROWS_X="450 1100" ABOUT_DARK_SUM=330 ABOUT_MIN_DARK=800
dark=$(grim -t ppm - | python3 -c '
import sys
y0, y1, x0, x1, dark_sum = map(int, sys.argv[1:6])
data = sys.stdin.buffer.read()
parts = data.split(maxsplit=4)
width, height = int(parts[1]), int(parts[2])
pixels = parts[4]
count = 0
for y in range(y0, min(y1, height)):
    row = y * width * 3
    for x in range(x0, min(x1, width)):
        r, g, b = pixels[row + 3 * x: row + 3 * x + 3]
        count += (r + g + b) < dark_sum
print(count)' $ABOUT_ROWS_Y $ABOUT_ROWS_X $ABOUT_DARK_SUM)
echo "pixels escuros nas linhas do Sobre: $dark"
[ "${dark:-0}" -gt "$ABOUT_MIN_DARK" ] && ok "o Sobre mostra as informações" || fail "o Sobre está vazio: $dark pixels escuros"

# 6a. O Aparência e o Telas sem o pacote de que dependem: saem com 1, dizem o que falta, e não mexem na janela
# (o aviso diz o pacote a instalar: o swaydesk-theme vem no swaydesk-theme-tool)
for triple in "appearance:swaydesk-theme:swaydesk-theme-tool" "displays:swaydesk-displays:swaydesk-displays"; do
  id=${triple%%:*} rest=${triple#*:}
  program=${rest%%:*} package=${rest#*:}
  PATH=$bare_path command -v "$program" > /dev/null && { fail "o $program ficou no PATH sem ele"; continue; }
  before=$(title)
  out=$(PATH=$bare_path timeout 10 "$bin" --panel "$id" 2>&1); rc=$?
  if [ "$rc" = 1 ] && printf '%s' "$out" | grep -q "$package is not installed"; then ok "--panel $id sem o $program sai com 1 e pede o $package"; else fail "--panel $id sem o $program: rc=$rc, saída: $out"; fi
  sleep 0.3
  [ "$(con_id)" = "$first" ] && [ "$(title)" = "$before" ] && ok "a janela não mudou" || fail "a janela mudou: '$(title)' (antes '$before'), con_id $(con_id)"
done

# 6. Um painel de fora sem o programa: sai com 1, diz o que falta, e não abre nada
if ! command -v pavucontrol > /dev/null; then
  before=$(title)
  out=$(timeout 10 "$bin" --panel sound 2>&1); rc=$?
  if [ "$rc" = 1 ] && printf '%s' "$out" | grep -q pavucontrol; then ok "--panel sound sem o pavucontrol sai com 1"; else fail "--panel sound: rc=$rc, saída: $out"; fi
  sleep 0.3
  [ "$(con_id)" = "$first" ] && [ "$(title)" = "$before" ] && ok "a janela não mudou" || fail "a janela mudou: '$(title)' (antes '$before'), con_id $(con_id)"
fi

# 8. O painel Desktop, com cliques do ponteiro virtual. As posições valem para a saída de 1280x800, a janela
# lado a lado (a tela toda), a barra lateral de 220 px e a página no topo, nesta ordem de cliques (a linha do OSD
# perde o "Definido pelo sistema" ao ser clicada e encolhe: ela vem depois das que estão abaixo dela). Quem mudar a
# página tira um grim e mede de novo (os prints settings-4-* ficam no /out)
DOCK_SWITCH="989 130" DOCK_RESET="934 130" OSD_SWITCH="989 281" WEATHER_SWITCH="989 541"
PROFILE_DROPDOWN="976 757" PROFILE_I3="948 608" PROFILE_SWAYDESK="1020 573"  # com a página rolada até o fim
toml() { python3 -c 'import sys, tomllib; print(eval(sys.argv[1], {"t": tomllib.load(open(sys.argv[2], "rb"))}))' "$1" "$config_toml" 2>&1; }
waited() { for _ in $(seq $ticks); do grep -qxF "$1" /tmp/calls && return 0; sleep 0.1; done; return 1; }
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
pointer() { /bin/swaydesk-vpointer --size 1280x800 "$@"; }
timeout 10 "$bin" --panel desktop >> /tmp/app.log 2>&1
wait_title "Desktop — Settings" && ok "--panel desktop" || fail "--panel desktop: '$(title)'"
sleep 0.5
grim /out/settings-4-desktop.png 2>/dev/null
# Um módulo da barra: grava o [enabled.bar] e reabre a Waybar pelo sway-waybar restart
pointer move $WEATHER_SWITCH sleep 100 click sleep 500
[ "$(toml 't["enabled"]["bar"]["weather"]')" = False ] && ok "desligar o Tempo grava enabled.bar.weather = false" || fail "Tempo: $(cat "$config_toml")"
waited "sway-waybar restart" && ok "a barra reabre pelo sway-waybar restart" || fail "sem o sway-waybar restart: $(cat /tmp/calls)"
# Dois cliques durante um restart (que leva 1 s): um restart por vez, e mais um depois, que lê o arquivo final
# (dois juntos subiriam duas Waybars)
printf '#!/bin/sh\necho start >> /tmp/restarts; sleep 1; echo end >> /tmp/restarts\n' > "$HOME/.local/bin/sway-waybar"
: > /tmp/restarts
pointer move $WEATHER_SWITCH sleep 100 click sleep 150 click sleep 150 click sleep 3500
restarts=$(tr '\n' ' ' < /tmp/restarts)
[ "$restarts" = "start end start end " ] && ok "um restart da barra por vez, e um a mais pelos cliques do meio" \
  || fail "restarts da barra: '$restarts'"
[ "$(toml 't["enabled"]["bar"]["weather"]')" = True ] && ok "três cliques: o arquivo fica com o último" || fail "Tempo depois de três cliques: $(cat "$config_toml")"
pointer move $WEATHER_SWITCH sleep 100 click sleep 1500
printf '#!/bin/sh\necho "sway-waybar $*" >> /tmp/calls\n' > "$HOME/.local/bin/sway-waybar"
# O interruptor da dock (false do usuário): liga, e os comentários ficam
pointer move $DOCK_SWITCH sleep 100 click sleep 500
[ "$(toml 't["enabled"]["dock"]')" = True ] && ok "ligar a dock grava dock = true" || fail "dock: $(cat "$config_toml")"
grep -qxF '# mine' "$config_toml" && grep -qF 'dock = true  # no dock' "$config_toml" && ok "os comentários do config.toml ficam" \
  || fail "comentários perdidos: $(cat "$config_toml")"
# O "Usar o padrão" tira a chave do usuário
pointer move $DOCK_RESET sleep 100 click sleep 500
[ "$(toml '"dock" in t["enabled"]')" = False ] && ok "Usar o padrão tira a chave da dock" || fail "dock ficou: $(cat "$config_toml")"
# O OSD desligado pela camada do sistema aparece desligado: o clique o liga, numa chave do usuário
pointer move $OSD_SWITCH sleep 100 click sleep 500
[ "$(toml 't["enabled"]["osd"]')" = True ] && ok "o OSD desligado pelo sistema liga com uma chave do usuário" || fail "osd: $(cat "$config_toml")"
grep -q osd /tmp/etc/system.toml && [ "$(grep -c . /tmp/etc/system.toml)" = 2 ] && ok "a camada do sistema não muda" || fail "o /etc mudou"
# O perfil de atalhos: grava e chama o desktop-config keys
pointer move 750 400 sleep 100 scroll 40 sleep 600 move $PROFILE_DROPDOWN sleep 100 click sleep 700 move $PROFILE_I3 sleep 100 click sleep 500
grim /out/settings-4-desktop-bottom.png 2>/dev/null
[ "$(toml 't["shortcuts"]')" = i3 ] && ok "o perfil i3 grava shortcuts = \"i3\"" || fail "shortcuts: $(cat "$config_toml")"
waited "sway-desktop-config keys" && ok "os atalhos são regravados (desktop-config keys)" || fail "sem o desktop-config keys: $(cat /tmp/calls)"
# Sem o desktop-config (sem o swaydesk-theme-tool), ninguém grava o 00-keys.conf: a seção diz o que falta e não
# oferece a escolha (a página é conferida de novo ao voltar a ela). O perfil fica o i3 de antes, sem o keys
rm "$HOME/.local/bin/sway-desktop-config"; : > /tmp/calls
timeout 10 "$bin" --panel region >> /tmp/app.log 2>&1; wait_title "Region — Settings"
timeout 10 "$bin" --panel desktop >> /tmp/app.log 2>&1; wait_title "Desktop — Settings"; sleep 1
pointer move 750 400 sleep 100 scroll 40 sleep 1000 move 1000 757 sleep 300 click sleep 1000
grim /out/settings-4-desktop-no-theme-tool.png 2>/dev/null
pointer move $PROFILE_SWAYDESK sleep 100 click sleep 500
[ "$(toml 't["shortcuts"]')" = i3 ] && ! grep -q "sway-desktop-config" /tmp/calls \
  && ok "sem o swaydesk-theme-tool, o perfil de atalhos não muda" || fail "sem o desktop-config, o perfil mudou: $(cat "$config_toml")"
printf '#!/bin/sh\necho "sway-desktop-config $*" >> /tmp/calls\n' > "$HOME/.local/bin/sway-desktop-config"
chmod +x "$HOME/.local/bin/sway-desktop-config"
timeout 10 "$bin" --panel region >> /tmp/app.log 2>&1; wait_title "Region — Settings"
timeout 10 "$bin" --panel desktop >> /tmp/app.log 2>&1; wait_title "Desktop — Settings"; sleep 0.5
# Um config.toml inválido fica como está: o clique não grava nada
printf 'dock = [\n' > "$config_toml"
pointer move 750 400 sleep 100 scroll -40 sleep 600 move $DOCK_SWITCH sleep 100 click sleep 500
[ "$(cat "$config_toml")" = 'dock = [' ] && ok "um config.toml inválido não é tocado" || fail "o inválido mudou: $(cat "$config_toml")"
grim /out/settings-4-desktop-invalid.png 2>/dev/null

# 9. O painel Região: os campos com o teclado (wtype), o idioma pela lista. As posições são as da página no topo,
# sem a mensagem de erro (ela empurra as linhas para baixo: o caso do erro vem por último)
LANGUAGE="976 78" CITY="975 137" HOLIDAYS="975 196" SPANISH="935 233"  # o Español, o 4º item da lista aberta
printf '# mine\nweather_location = "Porto"  # here\n' > "$config_toml"
# Um teclado que fica: um wtype novo manda a tecla antes de o cliente ligar o wl_keyboard
wtype -s 3600000 -k F24 & kbd=$!
keys() { wtype "$@"; sleep 0.3; }
timeout 10 "$bin" --panel region >> /tmp/app.log 2>&1
wait_title "Region — Settings" && ok "--panel region" || fail "--panel region: '$(title)'"
sleep 0.5
grim /out/settings-5-region.png 2>/dev/null
pointer move $CITY sleep 100 click sleep 300; keys -M ctrl a -m ctrl; keys Lisboa; keys -k Return
[ "$(toml 't["weather_location"]')" = Lisboa ] && grep -q '# here' "$config_toml" && grep -q '# mine' "$config_toml" \
  && ok "a cidade grava weather_location, e os comentários ficam" || fail "cidade: $(cat "$config_toml")"
pointer move $HOLIDAYS sleep 100 click sleep 300; keys pt; keys -k Tab
[ "$(toml 't["holidays"]')" = PT ] && ok "o país sai ao deixar o campo, em maiúsculas (pt vira PT)" || fail "feriados: $(cat "$config_toml")"
pointer move $CITY sleep 100 click sleep 300; keys -M ctrl a -m ctrl; keys -k BackSpace; keys -k Return
[ "$(toml '"weather_location" in t')" = False ] && ok "a cidade vazia tira a chave (vale a do IP)" || fail "cidade vazia: $(cat "$config_toml")"
pointer move $LANGUAGE sleep 100 click sleep 700 move $SPANISH sleep 100 click sleep 500
[ "$(toml 't.get("locale")')" = es_ES ] && ok "o idioma Español grava locale = \"es_ES\"" || fail "idioma: $(cat "$config_toml")"
grim /out/settings-5-region-language.png 2>/dev/null
# A lista fecha ao escolher: o campo da cidade fica sob o "English" dela, e um clique ali com a lista aberta
# trocaria o idioma em vez de ir ao campo
pointer move $CITY sleep 100 click sleep 300; keys Faro; keys -k Return
[ "$(toml 't.get("weather_location")')" = Faro ] && [ "$(toml 't.get("locale")')" = es_ES ] \
  && ok "a lista do idioma fecha ao escolher" || fail "a lista seguiu aberta: $(cat "$config_toml")"
before=$(cat "$config_toml")
pointer move $HOLIDAYS sleep 100 click sleep 300; keys -M ctrl a -m ctrl; keys Brazil; keys -k Return
[ "$(cat "$config_toml")" = "$before" ] && ok "um país que não é código não é gravado" || fail "Brazil gravou: $(cat "$config_toml")"
grim /out/settings-5-region-error.png 2>/dev/null

# 10. O painel Energia e bloqueio: as listas dos tempos com cliques nos itens (Never, 1, 2, 5, 10, 15, 30, 45, 60
# min, 35 px cada; cada lista abre logo abaixo da sua linha, com o Never 50 px abaixo dela). As setas do teclado
# não servem: o foco inicial da lista varia (o primeiro Down às vezes só a foca). O sway-idle falso (o do swaydesk-caffeine) anota cada vez que roda. Sem o
# power-profiles-daemon no contêiner, a seção do perfil não aparece
LOCK_LIST="972 129" SCREEN_LIST="972 184" SUSPEND_LIST="972 239"
LOCK_5MIN="965 284" SCREEN_2MIN="965 304" SUSPEND_NEVER="965 289"
printf '#!/bin/sh\necho "sway-idle ran" >> /tmp/calls\n' > "$HOME/.local/bin/sway-idle"
chmod +x "$HOME/.local/bin/sway-idle"
: > /tmp/calls
timeout 10 "$bin" --panel power >> /tmp/app.log 2>&1
wait_title "Power and lock — Settings" && ok "--panel power" || fail "--panel power: '$(title)'"
sleep 0.5
grim /out/settings-6-power.png 2>/dev/null
pointer move $LOCK_LIST sleep 100 click sleep 700 move $LOCK_5MIN sleep 100 click sleep 500
[ "$(toml 't.get("lock_after")')" = 5 ] && ok "bloquear em 5 min grava lock_after = 5 (um número)" || fail "lock_after: $(cat "$config_toml")"
waited "sway-idle ran" && ok "o sway-idle roda de novo para valer na hora" || fail "sem o sway-idle: $(cat /tmp/calls)"
pointer move $SCREEN_LIST sleep 100 click sleep 700; grim /out/settings-6-power-list.png 2>/dev/null
pointer move $SCREEN_2MIN sleep 100 click sleep 500
[ "$(toml 't.get("screen_off_after")')" = 2 ] && ok "apagar em 2 min grava screen_off_after = 2" || fail "screen_off_after: $(cat "$config_toml")"
grim /out/settings-6-power-hint.png 2>/dev/null
pointer move $SUSPEND_LIST sleep 100 click sleep 700 move $SUSPEND_NEVER sleep 100 click sleep 500
[ "$(toml 't.get("suspend_after")')" = 0 ] && ok "suspender Never grava suspend_after = 0" || fail "suspend_after: $(cat "$config_toml")"
# O perfil de energia (o daemon falso, que chegou numa thread): abre no ativo e grava pelo busctl set-property
PROFILE_LIST="964 397"  # abaixo do aviso da tela, que está visível aqui
PERFORMANCE="972 517"  # o 3º item da lista aberta (Power saver, Balanced, Performance)
pointer move $PROFILE_LIST sleep 100 click sleep 700
grim /out/settings-6-power-profiles.png 2>/dev/null
pointer move $PERFORMANCE sleep 100 click sleep 700
grep -q 'set-property net.hadess.PowerProfiles /net/hadess/PowerProfiles net.hadess.PowerProfiles ActiveProfile s performance' /tmp/calls \
  && ok "o perfil Performance vai ao daemon pelo busctl set-property" || fail "sem o set-property do perfil: $(cat /tmp/calls)"
# Sem o sway-idle (sem o swaydesk-caffeine): a seção diz o que falta; o painel é remontado ao voltar a ele
rm "$HOME/.local/bin/sway-idle"
timeout 10 "$bin" --panel region >> /tmp/app.log 2>&1; wait_title "Region — Settings"
timeout 10 "$bin" --panel power >> /tmp/app.log 2>&1; wait_title "Power and lock — Settings"; sleep 0.5
grim /out/settings-6-power-missing.png 2>/dev/null
before=$(cat "$config_toml")
pointer move $LOCK_LIST sleep 100 click sleep 500 move $LOCK_5MIN sleep 100 click sleep 500
[ "$(cat "$config_toml")" = "$before" ] && ok "sem o swaydesk-caffeine, não há lista para mudar" || fail "gravou sem o sway-idle: $(cat "$config_toml")"
kill $kbd 2>/dev/null
kill $keeper 2>/dev/null

# 11. O painel Apps padrão: o padrão do sistema vem da camada do /etc/xdg, e a escolha vai só para o
# ~/.config/mimeapps.list do usuário, sem mexer nas linhas de outros apps. As posições são as da página no topo
APPS_WEB="976 129" APPS_FILES="934 239" APPS_TERMINAL="942 294" APPS_IMAGES="900 404"
# As listas abrem embaixo do botão, que muda de largura com o nome escolhido: x no meio das duas posições
FAKE_BROWSER="900 214" THUNAR="930 359" FOOT="930 379" FIRST_IMAGE="960 454" FAKE_PHOTOS="960 524"
printf '[Added Associations]\ntext/plain=gedit.desktop;\n' > "$mimeapps"
system_before=$(cat /tmp/xdg-etc/mimeapps.list)
mime_line() { python3 - "$1" "$mimeapps" <<'PY'
import sys
inside = False
for line in open(sys.argv[2]):
    line = line.strip()
    if line.startswith("["):
        inside = line == "[Default Applications]"
    elif inside and line.split("=", 1)[0] == sys.argv[1]:
        print(line.split("=", 1)[1])
PY
}
timeout 10 "$bin" --panel default-apps >> /tmp/app.log 2>&1
wait_title "Default apps — Settings" && ok "--panel default-apps" || fail "--panel default-apps: '$(title)'"
sleep 0.5
grim /out/settings-7-apps.png 2>/dev/null
# Imagens: o Fake Photos só abre PNG, e só o PNG vai para ele (o JPEG segue o do sistema)
pointer move $APPS_IMAGES sleep 100 click sleep 700 move $FAKE_PHOTOS sleep 100 click sleep 700
[ "$(mime_line image/png)" = "fake-photos.desktop;" ] && [ -z "$(mime_line image/jpeg)" ] \
  && ok "Imagens: o PNG vai para o Fake Photos, o JPEG não" || fail "imagens: $(cat "$mimeapps")"
grep -qxF 'text/plain=gedit.desktop;' "$mimeapps" && ok "a linha de outro app fica" || fail "a linha do gedit sumiu: $(cat "$mimeapps")"
[ "$(cat /tmp/xdg-etc/mimeapps.list)" = "$system_before" ] && ok "a camada do sistema fica como estava" || fail "o /etc/xdg mudou: $(cat /tmp/xdg-etc/mimeapps.list)"
# De volta ao padrão do sistema: a linha do usuário sai
pointer move $APPS_IMAGES sleep 100 click sleep 700 move $FIRST_IMAGE sleep 100 click sleep 700
[ -z "$(mime_line image/png)" ] && ok "o padrão do sistema tira a escolha do usuário" || fail "image/png ficou: $(cat "$mimeapps")"
grim /out/settings-7-apps-system.png 2>/dev/null
# O navegador: os tipos da web vão juntos
pointer move $APPS_WEB sleep 100 click sleep 700 move $FAKE_BROWSER sleep 100 click sleep 700
[ "$(mime_line x-scheme-handler/http)$(mime_line x-scheme-handler/https)$(mime_line text/html)" = "fake-browser.desktop;fake-browser.desktop;fake-browser.desktop;" ] \
  && ok "o navegador fica com http, https e text/html" || fail "navegador: $(cat "$mimeapps")"
# O terminal vai para a chave terminal do config.toml
pointer move $APPS_TERMINAL sleep 100 click sleep 700 move $FOOT sleep 100 click sleep 700
[ "$(toml 't.get("terminal")')" = foot ] && ok "o terminal grava terminal = \"foot\"" || fail "terminal: $(cat "$config_toml")"
# O gerenciador de arquivos vai para o file_manager, e o generate o leva ao sway-mimeapps.list
: > /tmp/calls
pointer move $APPS_FILES sleep 100 click sleep 700
grim /out/settings-7-apps-files.png 2>/dev/null
pointer move $THUNAR sleep 100 click sleep 700
[ "$(toml 't.get("file_manager")')" = thunar ] && ok "o gerenciador grava file_manager = \"thunar\"" || fail "file_manager: $(cat "$config_toml")"
waited "sway-desktop-config generate" && ok "e roda o desktop-config generate" || fail "sem o generate: $(cat /tmp/calls)"
grim /out/settings-7-apps-chosen.png 2>/dev/null

# 7. Fechar a janela encerra o app
swaymsg "[app_id=\"$app_id\"] kill" > /dev/null
wait_windows 0 || fail "a janela não fechou"
for _ in $(seq $ticks); do kill -0 "$app" 2>/dev/null || break; sleep 0.1; done
kill -0 "$app" 2>/dev/null && fail "o app seguiu no ar sem janela" || ok "fechar a janela encerra o app"

# 8. O 80-input.conf dos painéis Teclado e Mouse e touchpad: o Sway aceita os arquivos que o render() grava
# (o teste input::tests::the_files_checked_by_sway_are_what_render_writes os mantém iguais). O sway -C sai com 0
# mesmo com erro num include: o que conta é não ter linha de erro do config
for conf in /repo/rust/settings-logic/tests/80-input-*.conf; do
    printf 'include %s\n' "$conf" > /tmp/input-check.conf
    sway -C -c /tmp/input-check.conf > /tmp/input-check.log 2>&1
    rc=$?
    if [ $rc = 0 ] && ! grep -q "\[sway/config.c" /tmp/input-check.log; then
        ok "sway -C aceita $(basename "$conf")"
    else
        fail "sway -C recusa $(basename "$conf") (rc $rc)"; cat /tmp/input-check.log
    fi
done
# O controle: um erro de sintaxe num include aparece no log (senão o caso acima não testaria nada)
printf 'input type:keyboard {\n    xkb_layout "br"\n    no_such_key x\n}\n' > /tmp/input-bad.conf
printf 'include /tmp/input-bad.conf\n' > /tmp/input-check.conf
sway -C -c /tmp/input-check.conf > /tmp/input-check.log 2>&1
grep -q "\[sway/config.c" /tmp/input-check.log && ok "sway -C acusa um erro num include" || fail "sway -C não acusou o erro de controle"

# 9. O painel Teclado: grava só o que muda no 80-input.conf e aplica ao vivo (o teclado virtual do wtype é o
# teclado do GET_INPUTS); "Use the system keyboard" tira os layouts. O config deste Sway não lê o config.d: a dica
# do include aparece (o print)
input_conf=$HOME/.config/sway/config.d/80-input.conf
ADD_FIRST="420 307" SECOND_UP="918 224" USE_SYSTEM="932 118"
kbd_inputs() { swaymsg -r -t get_inputs | python3 -c '
import json, sys
for i in json.load(sys.stdin):
    if i["type"] == "keyboard":
        print(i.get(sys.argv[1]))
        break' "$1"; }
# O teclado virtual do wtype tem o keymap dele (o Sway não lhe aplica o xkb_layout): os layouts se conferem pelo
# que o Sway respondeu ao comando do app, por um swaymsg que anota a chamada e o código de saída
mkdir -p /tmp/input-bin
printf '#!/bin/sh\n/usr/bin/swaymsg "$@"; rc=$?\necho "rc=$rc $*" >> /tmp/swaymsg.log\nexit $rc\n' > /tmp/input-bin/swaymsg
chmod +x /tmp/input-bin/swaymsg
: > /tmp/swaymsg.log
sent() { for _ in $(seq $ticks); do grep -qF -- "rc=0 $1" /tmp/swaymsg.log && return 0; sleep 0.1; done; return 1; }
waited_file() { for _ in $(seq $ticks); do grep -qF -- "$1" "$input_conf" 2>/dev/null && return 0; sleep 0.1; done; return 1; }
/bin/swaydesk-vpointer sleep 3600000 & keeper=$!
wtype -s 3600000 -k F24 & kbd=$!
sleep 0.3
PATH=/tmp/input-bin:$PATH "$bin" --panel keyboard >> /tmp/app.log 2>&1 &
app=$!
wait_title "Keyboard — Settings" && ok "--panel keyboard" || fail "--panel keyboard: '$(title)'"
sleep 1
grim /out/settings-9-keyboard.png 2>/dev/null
[ ! -e "$input_conf" ] && ok "abrir o painel não grava nada" || fail "o painel gravou ao abrir: $(cat "$input_conf")"
pointer move 989 569 sleep 100 click sleep 300
waited_file "xkb_numlock enabled" && ok "o Num Lock grava xkb_numlock enabled" || fail "Num Lock: $(cat "$input_conf" 2>&1)"
pointer move 996 450 sleep 100 click sleep 900
waited_file "repeat_delay 650" && ok "o + do atraso grava repeat_delay 650" || fail "atraso: $(cat "$input_conf" 2>&1)"
[ "$(kbd_inputs repeat_delay)" = 650 ] && ok "o atraso vale na hora (GET_INPUTS)" || fail "GET_INPUTS repeat_delay: $(kbd_inputs repeat_delay)"
grep -q "xkb_layout" "$input_conf" && fail "mudar o atraso gravou os layouts: $(cat "$input_conf")" || ok "o atraso não grava os layouts"
# O atraso e logo a velocidade, dentro da espera do campo (400 ms): cada um tem a sua, e os dois são gravados
pointer move 700 450 sleep 100 move 996 450 sleep 100 click sleep 100 move 996 509 sleep 100 click sleep 900
waited_file "repeat_delay 700" && waited_file "repeat_rate 26" && ok "atraso e velocidade seguidos gravam os dois" || fail "seguidos: $(cat "$input_conf" 2>&1)"
# Adicionar um layout pela busca: grava a lista toda e troca o keymap na hora
pointer move 535 213 sleep 100 click sleep 600
wtype "Portuguese (Brazil)"; sleep 0.6
grim /out/settings-9-keyboard-add.png 2>/dev/null
pointer move $ADD_FIRST sleep 100 click sleep 900
waited_file 'xkb_layout "us,br"' && ok "adicionar grava xkb_layout \"us,br\"" || fail "adicionar: $(cat "$input_conf" 2>&1)"
sent 'input type:keyboard xkb_layout "us,br"; input type:keyboard xkb_variant ","' && ok "o Sway aceita os layouts na hora" || fail "swaymsg: $(cat /tmp/swaymsg.log)"
grim /out/settings-9-keyboard-chosen.png 2>/dev/null
# Subir o segundo: a ordem muda
pointer move $SECOND_UP sleep 100 click sleep 900
waited_file 'xkb_layout "br,us"' && ok "subir grava xkb_layout \"br,us\"" || fail "subir: $(cat "$input_conf" 2>&1)"
pointer move $USE_SYSTEM sleep 100 click sleep 900
grep -q "xkb_layout" "$input_conf" && fail "Use the system keyboard deixou os layouts: $(cat "$input_conf")" || ok "Use the system keyboard tira os layouts"
waited_file "repeat_delay 700" && ok "e mantém o resto" || fail "o resto sumiu: $(cat "$input_conf" 2>&1)"
sent 'input type:keyboard xkb_layout "us"; input type:keyboard xkb_variant ""; input type:keyboard xkb_options ""' && ok "o teclado volta ao us na hora (sem arquivo nem XKB_DEFAULT_LAYOUT)" || fail "swaymsg: $(cat /tmp/swaymsg.log)"
grep -q "xkb_numlock" /tmp/swaymsg.log && fail "o Num Lock foi ao vivo: $(cat /tmp/swaymsg.log)" || ok "o Num Lock não vai ao vivo (o Sway o recusa fora do config)"

# 9c. O teclado do sistema vem do XKB_DEFAULT_LAYOUT (o Sway puro que o exporta), não do us: adicionar um layout
# parte dele. O app abre de novo com a variável (o que já está aberto não a vê) e volta sem ela, com o arquivo do 9,
# para o 10b
swaymsg "[app_id=\"$app_id\"] kill" > /dev/null
wait $app 2>/dev/null
cp "$input_conf" /tmp/80-input-before-9c.conf
rm -f "$input_conf"; : > /tmp/swaymsg.log
XKB_DEFAULT_LAYOUT=de PATH=/tmp/input-bin:$PATH "$bin" --panel keyboard >> /tmp/app.log 2>&1 &
app=$!
wait_title "Keyboard — Settings" || fail "--panel keyboard com XKB_DEFAULT_LAYOUT: '$(title)'"
sleep 1
grim /out/settings-9c-keyboard-env.png 2>/dev/null
pointer move 535 213 sleep 100 click sleep 600
wtype "Portuguese (Brazil)"; sleep 0.6
pointer move $ADD_FIRST sleep 100 click sleep 900
waited_file 'xkb_layout "de,br"' && ok "com XKB_DEFAULT_LAYOUT=de, adicionar grava xkb_layout \"de,br\"" \
  || fail "com XKB_DEFAULT_LAYOUT=de, adicionar: $(cat "$input_conf" 2>&1)"
swaymsg "[app_id=\"$app_id\"] kill" > /dev/null
wait $app 2>/dev/null
# O arquivo do 9 de volta: o 10b confere que o mouse não mexe no bloco do teclado
cp /tmp/80-input-before-9c.conf "$input_conf"
PATH=/tmp/input-bin:$PATH "$bin" --panel keyboard >> /tmp/app.log 2>&1 &
app=$!
wait_title "Keyboard — Settings" || fail "o app não voltou para o 10b: '$(title)'"

# 10b. O painel Mouse e touchpad: sem touchpad no contêiner, só a seção do mouse; a rolagem natural grava o
# input type:pointer, não toca no teclado
# Um bloco de um dispositivo só que dá uma chave do painel: o aviso aparece embaixo (o print)
printf 'input "1133:16500:Logitech_Mouse" left_handed enabled\n' > "$HOME/.config/sway/config.d/70-mine.conf"
timeout 10 "$bin" --panel mouse >> /tmp/app.log 2>&1
wait_title "Mouse and touchpad — Settings" && ok "--panel mouse" || fail "--panel mouse: '$(title)'"
sleep 1
grim /out/settings-10-mouse.png 2>/dev/null
kbd_block() { sed -n '/^input type:keyboard {/,/^}/p' "$input_conf"; }
kbd_before=$(kbd_block)
pointer move 989 193 sleep 100 click sleep 900
python3 - "$input_conf" <<'PY' && ok "a rolagem natural grava natural_scroll enabled no type:pointer" || fail "rolagem: $(cat "$input_conf")"
import re, sys
text = open(sys.argv[1]).read()
block = re.search(r"input type:pointer \{\n(.*?)\n\}", text, re.S)
sys.exit(0 if block and "natural_scroll enabled" in block.group(1) else 1)
PY
[ "$(kbd_block)" = "$kbd_before" ] && ok "o mouse não mexe no bloco do teclado" || fail "o bloco do teclado mudou: $(cat "$input_conf")"
rm -f "$HOME/.config/sway/config.d/70-mine.conf"

# 12. O painel Aparência: a página do app Tema (a biblioteca dele), montada na primeira vez que aparece. Os
# temas instalados e o tema do config antes de abrir o painel; o Arc-Dark (a 2ª linha, embaixo de "Other themes") e
# o Aplicar da linha de cima (no painel não há barra de título) gravam o tema. As posições valem para a janela na
# tela toda (1280x800) e a barra lateral de 220 px
ARC_DARK="262 212" APPLY="1234 50"
theme() {  # nome, fundo, texto, destaque
  mkdir -p "$HOME/.themes/$1/gtk-3.0"
  printf '@define-color theme_bg_color %s;\n@define-color theme_fg_color %s;\n@define-color theme_selected_bg_color %s;\n' \
    "$2" "$3" "$4" > "$HOME/.themes/$1/gtk-3.0/gtk.css"
}
theme Mint-Y-Dark-Teal "#2f2f2f" "#dadada" "#199ca8"
theme Mint-Y-Teal "#f0f0f0" "#303030" "#199ca8"
theme Arc-Dark "#383c4a" "#d3dae3" "#5294e2"
# A chave no topo: depois de uma tabela, ela seria da tabela
{ printf 'gtk_theme = "Mint-Y-Dark-Teal"\n'; cat "$config_toml"; } > /tmp/config.new && mv /tmp/config.new "$config_toml"
timeout 10 "$bin" --panel appearance >> /tmp/app.log 2>&1
wait_title "Appearance — Settings" && ok "--panel appearance" || fail "--panel appearance: '$(title)'"
sleep 1
grim /out/settings-11-appearance.png 2>/dev/null
pointer move $ARC_DARK sleep 100 click sleep 500 move $APPLY sleep 100 click sleep 100
for _ in $(seq $ticks); do [ "$(toml 't.get("gtk_theme")')" = Arc-Dark ] && break; sleep 0.1; done
sleep 1
grim /out/settings-11-appearance-applied.png 2>/dev/null
[ "$(toml 't.get("gtk_theme")')" = Arc-Dark ] && ok "o Aplicar do painel Aparência grava gtk_theme = \"Arc-Dark\"" || fail "gtk_theme: $(cat "$config_toml")"
# No painel, o Esc com a busca vazia não fecha a Central (na janela do app, fecha)
keys -k Escape; sleep 0.5
[ "$(windows)" = 1 ] && ok "o Esc no painel Aparência não fecha a Central" || fail "a Central fechou com o Esc"

# 13. O painel Telas: a página do app Telas. A prévia (o "Manter ou desfazer") segura uma trava no
# XDG_RUNTIME_DIR, e a janela do app e o painel são dois processos: com a trava num outro (o flock, no lugar de uma
# janela do app com a pergunta aberta), o Aplicar do painel não mexe na tela; com a pergunta do painel aberta,
# outro processo não pega a trava. As posições valem para a janela na tela toda (1280x800, uma saída)
DISPLAYS_SCALE_UP="1076 675" DISPLAYS_APPLY="1226 765"
preview_lock=$XDG_RUNTIME_DIR/sway-displays-preview.lock
scale() { swaymsg -r -t get_outputs | python3 -c 'import json, sys; print(round(json.load(sys.stdin)[0]["scale"], 2))'; }
asking() { swaymsg -t get_tree | grep -c '"name": "Keep these display settings?"'; }
timeout 10 "$bin" --panel displays >> /tmp/app.log 2>&1
wait_title "Displays — Settings" && ok "--panel displays" || fail "--panel displays: '$(title)'"
sleep 1
grim /out/settings-12-displays.png 2>/dev/null
# A trava num descritor do próprio script (um flock com um comando a passaria ao filho, que a seguraria depois
# do kill)
exec 9> "$preview_lock"
flock -n 9 || fail "a trava da prévia já estava presa"
pointer move $DISPLAYS_SCALE_UP sleep 100 click sleep 300 move $DISPLAYS_APPLY sleep 100 click sleep 1500
grim /out/settings-12-displays-refused.png 2>/dev/null
[ "$(scale)" = 1.0 ] && [ "$(asking)" = 0 ] && ok "com a prévia num outro processo, o Aplicar do painel não muda a tela" \
  || fail "o painel aplicou com a trava de outro: escala $(scale), $(asking) pergunta(s)"
exec 9>&-
pointer move $DISPLAYS_APPLY sleep 100 click
for _ in $(seq $ticks); do [ "$(asking)" = 1 ] && break; sleep 0.1; done
grim /out/settings-12-displays-asking.png 2>/dev/null
[ "$(asking)" = 1 ] && [ "$(scale)" = 1.05 ] && ok "sem a trava, o Aplicar do painel muda a tela e pergunta" || fail "Aplicar: escala $(scale), $(asking) pergunta(s)"
flock -n "$preview_lock" true && fail "outro processo pegou a trava com a pergunta do painel aberta" || ok "com a pergunta do painel aberta, outro processo não pega a trava"
keys -k Tab; keys -k Return; sleep 1  # na pergunta, o foco começa no Desfazer: o Tab vai ao Manter
[ "$(asking)" = 0 ] && [ "$(scale)" = 1.05 ] && grep -q '"scale": *1.05' "$HOME/.config/swaydesk/displays.json" 2>/dev/null \
  && ok "o Manter guarda o perfil (displays.json)" || fail "Manter: escala $(scale), $(asking) pergunta(s), $(cat "$HOME/.config/swaydesk/displays.json" 2>&1)"
flock -n "$preview_lock" true && ok "depois do Manter, a trava fica livre" || fail "a trava ficou presa depois do Manter"

swaymsg "[app_id=\"$app_id\"] kill" > /dev/null
wait_windows 0 || fail "a janela não fechou"
kill $kbd $keeper 2>/dev/null

grep -q "CRITICAL\|panicked" /tmp/app.log && { fail "avisos no log"; cat /tmp/app.log; }
cp /tmp/app.log /out/app.log 2>/dev/null
exit $failed
