#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o bloqueio em Rust (swaydesk-lock) num Sway headless, pelo caminho de verdade: sway-lock →
# sway-lock-watch → swaydesk-lock. Precisa do gtk4-layer-shell 1.2 ou mais novo (o Ubuntu 26.04; o
# Debian 13 tem o 1.0.4, sem o bloqueio). Roda num contêiner com o Sway e o grim, o repositório em /repo,
# o binário em /bin/swaydesk-lock e uma pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro -v $PWD:/repo:ro \
#     -v $PWD/rust/target/ubuntu2604/release/swaydesk-lock:/bin/swaydesk-lock:ro \
#     -v /tmp/out:/out swaydesk-test:ubuntu2604 /repo/rust/scripts/headless-lock.sh
#
# Os casos: o --check aceita a biblioteca; o sway-lock volta 0 com a tela bloqueada pelo swaydesk-lock (e
# não pelo Python nem pelo swaylock); um segundo sway-lock volta 0 ("F") sem abrir outro; dois monitores
# ganham a sua superfície; o locker encerrado é substituído por outro (o takeover do supervisor, que chama o
# sway-lock de novo), e o swaylock nunca é chamado.
# Digitar a senha fica para as VMs: o Sway headless não tem teclado virtual. Sai com 1 se algum falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
cp /bin/swaydesk-lock "$HOME/.local/bin/"
# Um swaylock falso: anota se alguém o chamar (ninguém pode)
printf '#!/bin/sh\necho "$@" >> /tmp/swaylock.calls\nexec sleep 3600\n' > /tmp/fakebin-swaylock
mkdir -p /tmp/fakebin && mv /tmp/fakebin-swaylock /tmp/fakebin/swaylock && chmod +x /tmp/fakebin/swaylock
# Um swaymsg que, com /tmp/hang, trava o get_inputs num neto (o sh fica esperando o sleep): o caso de um
# filho preso que herdasse o pipe do supervisor
real_swaymsg=$(command -v swaymsg)
cat > /tmp/fakebin/swaymsg <<EOF
#!/bin/sh
case "\$*" in *get_inputs*) [ -e /tmp/hang ] && { sleep 30; exit 1; } ;; esac
exec $real_swaymsg "\$@"
EOF
chmod +x /tmp/fakebin/swaymsg
export PATH=/tmp/fakebin:$PATH
# Um papel de parede laranja (um PNG feito à mão): o desfoque é gerado depois do locked, um por tamanho
mkdir -p "$HOME/.local/share/wallpaper"
python3 - "$HOME/.local/share/wallpaper/current.jpg" <<'PY'
import struct, sys, zlib
w, h = 64, 48
raw = b"".join(b"\0" + b"\xff\x80\x00" * w for _ in range(h))
chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
PY

export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=2 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720 position 0 0\noutput HEADLESS-2 resolution 1024x768 position 1280 0\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
eval "$(dbus-daemon --session --fork --print-address=1 | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"
sleep 0.5

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
lockers() { pgrep -x swaydesk-lock | wc -l; }

"$HOME/.local/bin/swaydesk-lock" --check 2>/tmp/check.log && ok "o --check aceita o gtk4-layer-shell daqui" || { fail "o --check recusou: $(cat /tmp/check.log)"; exit 1; }

start=$(date +%s%N)
SWAY_LOCK_IMPL=rust "$HOME/.local/bin/sway-lock"
rc=$?
took=$(( ($(date +%s%N) - start) / 1000000 ))
[ $rc = 0 ] && ok "o sway-lock volta 0 com a tela bloqueada (${took} ms)" || fail "o sway-lock voltou $rc"
[ "$(lockers)" = 1 ] && ok "um swaydesk-lock no ar" || fail "$(lockers) swaydesk-lock no ar"
[ -e /tmp/swaylock.calls ] && fail "o swaylock foi chamado: $(cat /tmp/swaylock.calls)" || ok "o swaylock não foi chamado"
pgrep -f sway-lock-gtk >/dev/null && fail "o sway-lock-gtk abriu" || ok "o Python não abriu"
sleep 2.5  # o desfoque (sem papel de parede: o fundo liso) e o estado do canto
grim -o HEADLESS-1 /out/lock-1.png 2>/dev/null
grim -o HEADLESS-2 /out/lock-2.png 2>/dev/null
[ -s /out/lock-1.png ] && [ -s /out/lock-2.png ] && ok "capturas dos dois monitores em /out" || fail "sem captura"

echo "$(date +%T.%N) segundo sway-lock" >> "$XDG_RUNTIME_DIR/sway-lock.log"
SWAY_LOCK_IMPL=rust "$HOME/.local/bin/sway-lock"
rc=$?
[ $rc = 0 ] && ok "um segundo sway-lock volta 0 (já bloqueado)" || fail "o segundo sway-lock voltou $rc"
sleep 1.2
[ "$(lockers)" = 1 ] && ok "o segundo não ficou no ar" || fail "$(lockers) swaydesk-lock no ar"

# O monitor que sai leva a janela; o que fica continua bloqueado
echo "$(date +%T.%N) desligando HEADLESS-2" >> "$XDG_RUNTIME_DIR/sway-lock.log"
swaymsg output HEADLESS-2 disable >/dev/null
sleep 0.5
[ "$(lockers)" = 1 ] && ok "desligar um monitor não derruba o bloqueio" || fail "o bloqueio caiu ao desligar um monitor"
echo "$(date +%T.%N) religando HEADLESS-2" >> "$XDG_RUNTIME_DIR/sway-lock.log"
swaymsg output HEADLESS-2 enable >/dev/null
sleep 1
[ "$(lockers)" = 1 ] && ok "religar o monitor não derruba o bloqueio" || fail "o bloqueio caiu ao religar o monitor"
grim -o HEADLESS-2 /out/lock-2-de-novo.png 2>/dev/null

# O locker cai com um swaymsg preso (o get_inputs travado num neto): o supervisor vê o fim do pipe na hora
touch /tmp/hang
sleep 2.5
pgrep -f "sleep 30" >/dev/null && ok "um get_inputs preso no ar" || fail "o get_inputs falso não travou"
first=$(pgrep -x swaydesk-lock)
pkill -KILL -x swaydesk-lock
second=
for _ in $(seq 40); do
  second=$(pgrep -x swaydesk-lock | grep -vx "$first")
  [ -n "$second" ] && break
  sleep 0.1
done
[ -n "$second" ] && ok "o locker morto é substituído por outro (o takeover), mesmo com um filho preso" || fail "o supervisor não passou o bloqueio a outro locker (o pipe vazou para um filho?)"
grep -q "takes over" "$XDG_RUNTIME_DIR/sway-lock.log" && ok "o takeover no log" || fail "o takeover não está no log"
[ -e /tmp/swaylock.calls ] && fail "o swaylock foi chamado: $(cat /tmp/swaylock.calls)" || ok "o swaylock não foi chamado no takeover"
rm -f /tmp/hang
grep -q "panicked" "$XDG_RUNTIME_DIR/sway-lock.log" && fail "pânico no log"
# Os CRITICAL de dentro do gtk4-layer-shell (o locker recusado e a troca de monitor) aparecem, sem falhar
criticals=$(grep -c "CRITICAL" "$XDG_RUNTIME_DIR/sway-lock.log")
[ "$criticals" = 0 ] || printf 'aviso   %s CRITICAL no log (veja abaixo)\n' "$criticals"
blurs=$(ls "$HOME/.cache/swaydesk"/lock-*.png 2>/dev/null | wc -l)
[ "$blurs" = 2 ] && ok "o desfoque dos dois tamanhos no cache" || fail "$blurs fundos no cache (esperava 2)"
[ "$(grep -c "background 1024x768" "$XDG_RUNTIME_DIR/sway-lock.log")" -ge 2 ] && ok "o monitor religado também ganha o fundo" || fail "o monitor religado ficou sem o fundo"
grep -q "monitor HEADLESS-2 gone" "$XDG_RUNTIME_DIR/sway-lock.log" && ok "o monitor desligado sai da lista" || fail "o monitor desligado ficou na lista"
echo "--- sway-lock.log"; cat "$XDG_RUNTIME_DIR/sway-lock.log" 2>/dev/null
exit $failed
