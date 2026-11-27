#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o app Telas (swaydesk-displays) e o serviço de perfis num Sway headless com dois monitores. Roda dentro
# do contêiner swaydesk-smoke:debian13, com o repositório em /repo, o binário em /bin, o wtype em /bin e uma
# pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro -v $PWD:/repo:ro \
#     -v $PWD/rust/target/debian13/release/swaydesk-displays:/bin/swaydesk-displays:ro \
#     -v /caminho/do/wtype:/bin/wtype:ro -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-displays.sh
#
# O serviço: sem perfil, nada muda; com perfil, aplica ao subir; um monitor novo (create_output) muda o
# conjunto; tirá-lo (unplug) volta ao perfil do par; uma mudança à mão com o mesmo conjunto fica; o reload
# reaplica; sem nenhuma tela ligada, o apply liga todas. O app: Aplicar e Esc desfaz; Aplicar e o tempo esgotado desfaz; fechar a janela ou o SIGTERM com a pergunta aberta desfaz, e depois de um SIGKILL o app desfaz ao abrir, e o serviço no reload e no apply; Aplicar e Manter grava o perfil
# e o 90-displays.conf. Sai com 1 se algum caso falhar.
set -u
. /repo/rust/scripts/lib-wait.sh
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.config/sway/config.d" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=2 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x800 position 0 0\noutput HEADLESS-2 resolution 1024x768 position 1280 0\n' > /tmp/sway.conf
printf 'include %s/.config/sway/config.d/*\n' "$HOME" >> /tmp/sway.conf
sway -c /tmp/sway.conf > /out/sway.log 2>&1 &
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
eval "$(dbus-daemon --session --fork --print-address=1 | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"

failed=0
expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
# "HEADLESS-1:0,0 HEADLESS-2:1280,0": as ligadas, com a posição
layout() {
  swaymsg -r -t get_outputs | python3 -c 'import json,sys
print(" ".join(sorted("%s:%d,%d" % (o["name"], o["rect"]["x"], o["rect"]["y"]) for o in json.load(sys.stdin) if o["active"])))'
}
scale_of() { swaymsg -r -t get_outputs | python3 -c "import json,sys; print([o['scale'] for o in json.load(sys.stdin) if o['name']=='$1'][0])"; }
key_of() { swaymsg -r -t get_outputs | python3 -c "import json,sys
o=[o for o in json.load(sys.stdin) if o['name']=='$1'][0]
print(' '.join(x or 'Unknown' for x in (o['make'], o['model'], o['serial'])))"; }
wait_layout() {  # espera até 5 s pelo layout
  for _ in $(seq $ticks); do [ "$(layout)" = "$1" ] && break; sleep 0.1; done
  layout
}
profiles=$HOME/.config/swaydesk/displays.json
# Um perfil: pares "chave x y" das ligadas
write_profile() {
  python3 - "$profiles" "$@" <<'EOF'
import json, os, sys
path, args = sys.argv[1], sys.argv[2:]
outputs = {}
for i in range(0, len(args), 3):
    outputs[args[i]] = {"enabled": True, "position": [int(args[i + 1]), int(args[i + 2])], "scale": 1.0,
                        "transform": "normal", "adaptive_sync": False}
os.makedirs(os.path.dirname(path), exist_ok=True)
old = json.load(open(path)) if os.path.exists(path) else {"version": 1, "profiles": []}
old["profiles"].append({"outputs": outputs})
json.dump(old, open(path, "w"))
EOF
}
k1=$(key_of HEADLESS-1) k2=$(key_of HEADLESS-2)
# Dois monitores iguais sem número de série: a porta desempata
[ "$k1" = "$k2" ] && k1="$k1#HEADLESS-1" k2="$k2#HEADLESS-2"

# --- O serviço ---
/bin/swaydesk-displays daemon 2>>/out/daemon.log & daemon=$!
sleep 1
expect "sem perfil, o layout do Sway fica" "HEADLESS-1:0,0 HEADLESS-2:1280,0" "$(layout)"
kill $daemon; wait $daemon 2>/dev/null
write_profile "$k1" 1024 0 "$k2" 0 0
/bin/swaydesk-displays daemon 2>>/out/daemon.log & daemon=$!
expect "com perfil, aplica ao subir" "HEADLESS-1:1024,0 HEADLESS-2:0,0" "$(wait_layout 'HEADLESS-1:1024,0 HEADLESS-2:0,0')"
# Um segundo serviço sai na hora com o 4 (resident::EXIT_ANOTHER_INSTANCE): a unidade do systemd não o reinicia
# ao lado do primeiro (RestartPreventExitStatus=4), e o laço da sessão segue tentando
/bin/swaydesk-displays daemon 2>/out/second.log; expect "um segundo serviço sai na hora, com o 4" "4" "$?"
expect "e diz qual cópia está no ar" "1" "$(grep -c 'another instance is running' /out/second.log)"
cat /out/second.log >> /out/daemon.log
swaymsg -q create_output; sleep 1.5
expect "um monitor novo, sem perfil para o trio: nada muda nos dois" "HEADLESS-1:1024,0 HEADLESS-2:0,0 HEADLESS-3:2304,0" "$(layout)"
swaymsg -q output HEADLESS-1 pos 0 0, output HEADLESS-2 pos 1280 0, output HEADLESS-3 pos 2304 0
swaymsg -q output HEADLESS-3 unplug
expect "tirar o monitor volta ao perfil do par" "HEADLESS-1:1024,0 HEADLESS-2:0,0" "$(wait_layout 'HEADLESS-1:1024,0 HEADLESS-2:0,0')"
swaymsg -q output HEADLESS-1 pos 0 0, output HEADLESS-2 pos 1280 0; sleep 1.5
expect "uma mudança à mão com o mesmo conjunto fica" "HEADLESS-1:0,0 HEADLESS-2:1280,0" "$(layout)"
swaymsg -q reload
expect "o reload reaplica o perfil" "HEADLESS-1:1024,0 HEADLESS-2:0,0" "$(wait_layout 'HEADLESS-1:1024,0 HEADLESS-2:0,0')"
# A rede de segurança: um trio sem perfil, todas desligadas; o apply (o da tampa) liga todas
swaymsg -q create_output; sleep 1.5
# O nome novo: os números das saídas tiradas não voltam
extra=$(swaymsg -r -t get_outputs | python3 -c 'import json,sys; print([o["name"] for o in json.load(sys.stdin) if o["name"] not in ("HEADLESS-1","HEADLESS-2")][0])')
swaymsg -q output HEADLESS-1 disable, output HEADLESS-2 disable, output $extra disable; sleep 1
expect "as três desligadas" "0" "$(layout | wc -w)"
/bin/swaydesk-displays apply 2>>/out/daemon.log
expect "sem nenhuma tela ligada, o apply liga todas" "3" "$(layout | wc -w)"
# O apply também é chamado à mão e por scripts, não só pela tampa: o log diz "apply"
expect "o log do apply diz apply, não lid" "1 0" "$(grep -c 'turned every screen on (apply)' /out/daemon.log) $(grep -c '(lid)' /out/daemon.log)"
swaymsg -q output $extra unplug
expect "e o par volta ao perfil" "HEADLESS-1:1024,0 HEADLESS-2:0,0" "$(wait_layout 'HEADLESS-1:1024,0 HEADLESS-2:0,0')"
applied=$(grep -c "applied the profile" /out/daemon.log)
sleep 2
expect "sem laço: nada aplicado depois" "$applied" "$(grep -c "applied the profile" /out/daemon.log)"

# --- O app ---
windows() { swaymsg -t get_tree | grep -c '"app_id": "org.swaydesktop.Displays"'; }
app() { env GTK_A11Y=none G_DEBUG=fatal-criticals /bin/swaydesk-displays >> /out/app.log 2>&1 & }
# O teclado virtual novo de cada wtype perde a primeira tecla: um Shift vai antes
keys() { wtype -k Shift_L "$@"; }
# O foco começa na lista das telas; sem modos (as saídas headless), a Resolução e a Taxa ficam desligadas:
# Tab vai à Ligada e depois à Escala. Page_Up soma 0,25 à escala, e Ctrl+Enter aplica
set_scale_up() {
  keys -k Tab -k Tab; sleep 0.3
  keys -k Page_Up -k Page_Up; sleep 0.5
}
apply() { keys -M ctrl -k Return -m ctrl; }
app
for _ in $(seq $ticks); do [ "$(windows)" = 1 ] && break; sleep 0.1; done
sleep 1.5
expect "a janela abre" "1" "$(windows)"
grim /out/displays-1-open.png
app; sleep 1.5
expect "abrir de novo não abre outra" "1" "$(windows)"
# A primeira da lista do app é a primeira do Sway
first=$(swaymsg -r -t get_outputs | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["name"])')
before=$(scale_of $first)
set_scale_up; grim /out/displays-2-edited.png
apply; sleep 1.5
grim /out/displays-3-confirm.png
expect "Aplicar muda na hora" "1.5" "$(scale_of $first)"
keys -k Escape; sleep 1.5
expect "Esc desfaz" "$before" "$(scale_of $first)"
# A mudança continua na janela depois de desfazer: Aplicar de novo e esperar o tempo
apply; sleep 1.5
expect "Aplicar de novo" "1.5" "$(scale_of $first)"
sleep 17
expect "sem resposta em 15 s, volta sozinho" "$before" "$(scale_of $first)"
grim /out/displays-4-reverted.png
# A janela some com a pergunta aberta: o layout sem confirmar não pode ficar (uma tela apagada ou na escala errada
# é o caso para que a contagem existe). Fechar a janela pelo Sway (Super+Q) não a fecha enquanto a pergunta modal
# está aberta (a contagem segue); o SIGTERM desfaz na hora; depois de um SIGKILL (um crash), a próxima vez que o
# app abre desfaz pelo arquivo pendente do XDG_RUNTIME_DIR
pending=$XDG_RUNTIME_DIR/sway-displays-pending.json
app_gone() { for _ in $(seq $ticks); do pgrep -u "$(id -u)" -f '/bin/swaydesk-displays$' > /dev/null || return 0; sleep 0.1; done; return 1; }
reopen() { app; for _ in $(seq $ticks); do [ "$(windows)" = 1 ] && break; sleep 0.1; done; sleep 1.5; set_scale_up; }
apply; sleep 1.5
expect "Aplicar antes de fechar a janela" "1.5" "$(scale_of $first)"
swaymsg -q '[app_id="org.swaydesktop.Displays" title="^Displays$"] kill'; sleep 1
expect "fechar a janela pelo Sway com a pergunta aberta: a janela e a pergunta ficam" "2" "$(windows)"
keys -k Escape; sleep 1.5
expect "e a pergunta ainda desfaz" "$before no" "$(scale_of $first) $([ -e "$pending" ] && echo yes || echo no)"
apply; sleep 1.5
expect "Aplicar antes do SIGTERM" "1.5" "$(scale_of $first)"
pkill -u "$(id -u)" -TERM -f '/bin/swaydesk-displays$'
app_gone; sleep 1
expect "o SIGTERM com a pergunta aberta desfaz" "$before" "$(scale_of $first)"
reopen; apply; sleep 1.5
expect "Aplicar antes do SIGKILL" "1.5" "$(scale_of $first)"
pkill -u "$(id -u)" -KILL -f '/bin/swaydesk-displays$'
app_gone; sleep 1
expect "o SIGKILL não deixa desfazer (fica o arquivo pendente)" "1.5 yes" "$(scale_of $first) $([ -e "$pending" ] && echo yes || echo no)"
app; for _ in $(seq $ticks); do [ "$(windows)" = 1 ] && break; sleep 0.1; done; sleep 2.5
expect "a próxima vez que o app abre, desfaz o que ficou pendente" "$before no" "$(scale_of $first) $([ -e "$pending" ] && echo yes || echo no)"
grim /out/displays-4b-restored-on-start.png
# O serviço também consome o arquivo pendente (o app pode não abrir de novo, e a tela pode estar apagada): no
# reload e no apply (a tampa). Sem perfil para o par, só o arquivo desfaz; com a pergunta aberta, o serviço não mexe.
# O reload do Sway já volta a escala pelo config: nele, conta o arquivo consumido e a linha do serviço
mv "$profiles" "$profiles.bak"
undone() { grep -c "a preview was left without an answer: undoing" /out/daemon.log; }
set_scale_up; apply; sleep 1.5
swaymsg -q reload; sleep 1.5
expect "o reload com a pergunta aberta não consome o arquivo" "yes 0" "$([ -e "$pending" ] && echo yes || echo no) $(undone)"
pkill -u "$(id -u)" -KILL -f '/bin/swaydesk-displays$'
app_gone; sleep 1
swaymsg -q reload; sleep 2
expect "depois de um SIGKILL, o reload consome o arquivo pendente" "$before no 1" "$(scale_of $first) $([ -e "$pending" ] && echo yes || echo no) $(undone)"
reopen; apply; sleep 1.5
pkill -u "$(id -u)" -KILL -f '/bin/swaydesk-displays$'
app_gone; sleep 1
/bin/swaydesk-displays apply 2>>/out/daemon.log
expect "depois de um SIGKILL, o apply (a tampa) desfaz pelo arquivo pendente" "$before no" "$(scale_of $first) $([ -e "$pending" ] && echo yes || echo no)"
mv "$profiles.bak" "$profiles"
app; for _ in $(seq $ticks); do [ "$(windows)" = 1 ] && break; sleep 0.1; done; sleep 1.5
set_scale_up
apply; sleep 1.5
# Na pergunta, o foco começa no Desfazer: Tab vai ao Manter
keys -k Tab -k Return; sleep 1.5
expect "Manter fica" "1.5" "$(scale_of $first)"
grim /out/displays-5-kept.png
expect "Manter grava o perfil do par" "1.5" "$(python3 -c "import json
p=json.load(open('$profiles'))['profiles']
k='$k1' if '$first'=='HEADLESS-1' else '$k2'
print([x for x in p if sorted(x['outputs'])==sorted(['$k1','$k2'])][0]['outputs'][k]['scale'])")"
conf=$HOME/.config/sway/config.d/90-displays.conf
expect "Manter grava o 90-displays.conf" "1" "$(grep -c "output $first enable .* scale 1.5 " "$conf")"
# Um modo e uma escala postos à mão ficam até o reload (o app Tema faz um ao aplicar); aí o perfil volta
mode_of() { swaymsg -r -t get_outputs | python3 -c "import json,sys; m=[o['current_mode'] for o in json.load(sys.stdin) if o['name']=='$1'][0]; print(f\"{m['width']}x{m['height']}\")"; }
kept_mode=$(mode_of $first)
swaymsg -q output $first mode 800x600 scale 1; sleep 1.5
expect "o modo posto à mão fica sem reload" "800x600 1.0" "$(mode_of $first) $(scale_of $first)"
swaymsg -q reload; sleep 1.5
expect "o reload volta ao modo e à escala guardados" "$kept_mode 1.5" "$(mode_of $first) $(scale_of $first)"
# Com uma tela só, desligá-la é recusado com o aviso (a captura mostra a mensagem)
pkill -f '/bin/swaydesk-displays$'; sleep 0.5
other=$(swaymsg -r -t get_outputs | python3 -c "import json,sys; print([o['name'] for o in json.load(sys.stdin) if o['name']!='$first'][0])")
swaymsg -q output $other disable, output $first scale 1; sleep 1
app; sleep 2
# Da lista das telas (a primeira) ao interruptor Ligada, e espaço
keys -k Tab; sleep 0.3; keys -k space; sleep 1
grim /out/displays-6-last-screen.png
expect "a última tela ligada continua ligada" "1" "$(layout | wc -w)"
expect "nenhum aviso do GTK" "0" "$(grep -c -i 'critical\|warning' /out/app.log)"
kill $daemon 2>/dev/null
exit $failed
