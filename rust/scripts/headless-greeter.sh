#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa a tela de login (swaydesk-greeter) num Sway headless com dois monitores. Roda dentro do contêiner
# swaydesk-smoke:debian13 como root (para criar as contas), com o swaydesk-greeter e o wtype em /bin e a
# Symbols Nerd Font em /fonts; as capturas e os registros vão para /out. O greetd é o fake-greetd.py.
#
# - a tela abre com a conta lembrada (com a foto do AccountsService), a sessão do pacote primeiro e o formulário só no monitor de cima à esquerda;
# - a senha errada mostra o erro e cancela a sessão no greetd;
# - a certa abre a sessão escolhida (o comando e o ambiente do .desktop) e grava a memória;
# - uma conta com segundo fator recebe a pergunta visível depois da senha;
# - uma senha com acento: a tecla morta compõe no locale C.UTF-8 (o do Ubuntu Server);
# - outra cor de destaque pela config do sistema;
# - a cor que cada conta escolheu no app Tema (a do AccountsService, falso aqui) vence a da config do sistema, e
#   uma conta sem cor fica com a da config.
# Sai com 1 se algo falhar.
set -u
export XDG_RUNTIME_DIR=/tmp/xdg HOME=/root
mkdir -p -m 700 "$XDG_RUNTIME_DIR"
mkdir -p /usr/share/fonts/truetype/swaydesk && cp /fonts/*.ttf /usr/share/fonts/truetype/swaydesk/ && fc-cache -f >/dev/null
useradd -m -c "Ana Souza" -s /bin/bash ana
useradd -m -c "Beatriz Lima,,," -s /bin/bash bia
useradd -m -c "Conta com código" -s /bin/bash otp
useradd -m -c "Conta com acento" -s /bin/bash acento
useradd -r -s /usr/sbin/nologin servico
# A foto da ana pelo AccountsService (a que vale); o ~/.face dela nunca é lido pela tela
mkdir -p /var/lib/AccountsService/icons
python3 -c 'import zlib,struct
def chunk(t,d): return struct.pack(">I",len(d))+t+d+struct.pack(">I",zlib.crc32(t+d))
rows=b"".join(b"\0"+bytes([200,120,40])*120 for _ in range(120))
open("/var/lib/AccountsService/icons/ana","wb").write(b"\x89PNG\r\n\x1a\n"+chunk(b"IHDR",struct.pack(">IIBBBBB",120,120,8,2,0,0,0))+chunk(b"IDAT",zlib.compress(rows))+chunk(b"IEND",b""))'
mkdir -p /usr/share/wayland-sessions
printf '[Desktop Entry]\nName=Sway Desktop\nExec=/usr/libexec/swaydesk/sway-session --package\nDesktopNames=sway\n' > /usr/share/wayland-sessions/swaydesk.desktop
printf '[Desktop Entry]\nName=GNOME\nExec=gnome-session\nDesktopNames=GNOME\n' > /usr/share/wayland-sessions/gnome.desktop
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=2 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x800 position 0 0 bg #1b1b1f solid_color\noutput HEADLESS-2 resolution 1024x768 position 1280 0 bg #1b1b1f solid_color\ndefault_border none\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /out/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
export GREETD_SOCK=/tmp/greetd.sock SWAYDESK_GREETER_STATE=/tmp/state.json SWAYDESK_GREETER_CONFIG=/tmp/greeter.toml
python3 /repo/rust/scripts/fake-greetd.py "$GREETD_SOCK" /out/greetd.log & 
for _ in $(seq 50); do [ -S "$GREETD_SOCK" ] && break; sleep 0.1; done
failed=0
expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
start() {
  /bin/swaydesk-greeter 2>>/out/greeter.log & greeter=$!
  sleep "${1:-2}"
}
# Digita e dá Enter. O teclado virtual novo de cada wtype perde a primeira tecla: um Shift vai antes
type_line() { wtype -k Shift_L "$1" -k Return; }
gone() { for _ in $(seq 50); do kill -0 "$greeter" 2>/dev/null || return 0; sleep 0.1; done; return 1; }
requests() { python3 -c 'import json,sys; print(" ".join(json.loads(l)["type"] for l in open("/out/greetd.log")))'; }

: > /out/greetd.log
printf '{"last_user": "ana", "sessions": {"ana": "gnome"}}' > /tmp/state.json
start 3
grim /out/greeter-1-start.png
type_line errada; sleep 1.5
grim /out/greeter-2-wrong.png
expect "a senha errada cancela a sessão" "create_session post_auth_message_response cancel_session" "$(requests)"
kill -0 "$greeter" 2>/dev/null; expect "a tela continua depois do erro" "0" "$?"
: > /out/greetd.log
type_line a123
gone; expect "a senha certa fecha a tela" "0" "$?"
expect "a sessão lembrada (gnome) é a aberta" \
  '{"type": "start_session", "cmd": ["gnome-session"], "env": ["XDG_SESSION_TYPE=wayland", "XDG_SESSION_DESKTOP=gnome", "XDG_CURRENT_DESKTOP=GNOME"]}' \
  "$(grep start_session /out/greetd.log)"
expect "a memória guarda a conta" "ana gnome" "$(python3 -c 'import json; s=json.load(open("/tmp/state.json")); print(s["last_user"], s["sessions"]["ana"])')"

# Segundo fator: a senha e depois a pergunta visível
: > /out/greetd.log
printf '{"last_user": "otp"}' > /tmp/state.json
start 3
type_line a123; sleep 1.5
grim /out/greeter-3-code.png
kill -0 "$greeter" 2>/dev/null; expect "a pergunta do código fica na tela" "0" "$?"
type_line 42
gone; expect "o código certo abre a sessão (a do pacote, sem memória)" "0" "$?"
expect "o código foi a resposta da pergunta visível" '{"type": "post_auth_message_response", "response": "42"}' "$(grep '"42"' /out/greetd.log)"
expect "sem memória da sessão, a do pacote" '["/usr/libexec/swaydesk/sway-session", "--package"]' \
  "$(grep start_session /out/greetd.log | python3 -c 'import json,sys; print(json.dumps(json.load(sys.stdin)["cmd"]))')"

# Teclas mortas no locale C.UTF-8 (o do Ubuntu Server): ´ + e precisa virar é. O wtype manda os keysyms
: > /out/greetd.log
printf '{"last_user": "acento"}' > /tmp/state.json
start 3
wtype -k Shift_L caf -k dead_acute e -k Return
gone; expect "a tecla morta compõe no C.UTF-8 (café)" "0" "$?"
kill "$greeter" 2>/dev/null

# Outra cor, em português (sem o .mo, o texto fica em inglês: o teste é da cor)
printf 'accent = "#8e44ad"\n' > /tmp/greeter.toml
rm -f /tmp/state.json
start 3
grim /out/greeter-4-violet.png
kill "$greeter"

# A cor de cada conta, do AccountsService (o falso, num barramento próprio que a tela lê como o do sistema). Pela
# captura em PPM: os pixels exatamente da cor (o botão Entrar é dela, sem sombra)
pixels() { grim -t ppm "$1" && python3 - "$1" "$2" <<'PY'
import sys
data = open(sys.argv[1], "rb").read()
fields, pos = [], 0
while len(fields) < 4:
    while data[pos:pos + 1].isspace():
        pos += 1
    end = pos
    while not data[end:end + 1].isspace():
        end += 1
    fields.append(data[pos:end]); pos = end
body = data[pos + 1:]
color = bytes.fromhex(sys.argv[2].lstrip("#"))
print(sum(1 for i in range(0, len(body), 3) if body[i:i + 3] == color) > 2000)
PY
}
bus_address=$(dbus-daemon --session --fork --print-address) || failed=1
DBUS_SESSION_BUS_ADDRESS=$bus_address python3 /repo/rust/scripts/fake-accounts.py ana=#e11d48 bia=not-a-color > /tmp/accounts.ready &
for _ in $(seq 50); do grep -q ready /tmp/accounts.ready 2>/dev/null && break; sleep 0.1; done
export DBUS_SYSTEM_BUS_ADDRESS=$bus_address
printf '{"last_user": "ana"}' > /tmp/state.json
start 3
expect "a cor da conta (ana, #e11d48) vence a da config" "True" "$(pixels /out/greeter-5-ana.ppm '#e11d48')"
kill "$greeter"
printf '{"last_user": "bia"}' > /tmp/state.json
start 3
expect "um valor que não é cor (bia) fica com a da config" "True" "$(pixels /out/greeter-6-bia.ppm '#8e44ad')"
kill "$greeter"
expect "nenhum python3 é filho da tela" "" "$(ps -o comm= --ppid "$greeter" 2>/dev/null | grep python || true)"
exit $failed
