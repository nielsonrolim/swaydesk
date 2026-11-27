#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa a Ajuda do Swaydesk (swaydesk-help) num Sway headless. Roda dentro do contêiner swaydesk-smoke:debian13,
# com o repositório em /repo, o binário em /bin e uma pasta gravável em /out (as capturas):
#
#   docker run --rm --memory 10g --memory-swap 10g -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro \
#     -v $PWD:/repo:ro -v $PWD/rust/target/debian13/release/swaydesk-help:/bin/swaydesk-help:ro \
#     -v /tmp/out:/out swaydesk-smoke:debian13 /repo/rust/scripts/headless-help.sh
#
# O Sway do teste usa um config com descrições (## e ###), um include do config.d com um 50-local.conf que
# troca um atalho, e o $mod+F1 da ajuda. Os casos: o atalho abre a janela com o app_id certo; abrir de novo
# fecha; digitar busca; o Esc limpa a busca e, com ela vazia, fecha; o Ctrl+Tab abre a aba "Using Swaydesk". Sai com 1
# se algum caso falhar.
#
# Com "repo", usa o config do Sway do repositório (sem as linhas exec, que abririam a sessão inteira) e só tira
# capturas das duas abas, em pt_BR: confere as descrições, as seções e os chips do config de verdade, e o
# cartão da dock desligada ([enabled] dock = false).
#
# Com "profiles", os perfis de atalhos: três arquivos em ~/.config/sway/keys (swaydesk, i3, dwm) e o config.toml
# no dwm e depois no i3: a abertura com o seletor e, no dwm, o aviso (sem ponteiro, a troca fica para a VM). Por
# fim, como nos dotfiles: o ~/.config/sway/config é um link, e os perfis só existem ao lado do arquivo de verdade
# (keys_dir segue o link): o seletor aparece do mesmo jeito.
#
# Com "settings" (e no fim da rodada padrão), o cartão da Central de Configurações na aba "Using Swaydesk": sem ela, nenhum (captura
# settings-sem); com o .desktop dela e uma Central falsa no PATH (o TryExec), o cartão aparece primeiro (captura
# settings-com). O nome da falsa é montado em tempo de execução: o run-all-tests monta todo swaydesk-* citado aqui.
set -u
. /repo/rust/scripts/lib-wait.sh
mode=${1:-}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/sway/config.d" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do ln -sf "$f" "$HOME/.local/bin/"; done
cp /bin/swaydesk-help "$HOME/.local/bin/"

cat > "$HOME/.config/sway/config" <<'EOF'
output HEADLESS-1 resolution 1280x720
set $mod Mod4
set $bin ~/.local/bin
### Apps
## Open the terminal
bindsym Ctrl+Alt+t exec kitty
## Show the keyboard shortcuts
bindsym $mod+F1 exec $bin/sway-help
bindsym $mod+slash exec $bin/sway-help
### Workspaces
## Go to workspace
bindsym $mod+1 workspace number 1
bindsym $mod+2 workspace number 2
bindsym $mod+3 workspace number 3

bindsym --locked XF86AudioMute exec true
bindsym $mod+Shift+e exec $bin/sway-capture shot
include ~/.config/sway/config.d/*
EOF
cat > "$HOME/.config/sway/config.d/50-local.conf" <<'EOF'
## Open foot instead
bindsym --no-warn Ctrl+Alt+t exec foot
EOF
if [ "$mode" = profiles ]; then
  mkdir -p "$HOME/.config/sway/keys" "$HOME/.config/swaydesk"
  for p in swaydesk i3 dwm; do printf '### Windows\n## Close the window\nbindsym $mod+q kill\n' > "$HOME/.config/sway/keys/$p.conf"; done
  printf 'accent = "#199ca8"\nshortcuts = "dwm"\n' > "$HOME/.config/swaydesk/config.toml"
fi
if [ "$mode" = repo ]; then
  { echo 'output HEADLESS-1 resolution 1280x2000'; grep -v '^exec' /repo/home/.config/sway/config; } > "$HOME/.config/sway/config"
  rm "$HOME/.config/sway/config.d/50-local.conf"
  mkdir -p "$HOME/.config/swaydesk" && printf 'locale = "pt_BR"\n[enabled]\ndock = false\n' > "$HOME/.config/swaydesk/config.toml"
  mkdir -p "$HOME/.local/share" && ln -s /repo/home/.local/share/locale "$HOME/.local/share/locale"
fi

export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8 GTK_A11Y=none
# O app é único pelo D-Bus da sessão (GtkApplication): o barramento sobe antes do Sway, para o exec do
# config herdar o endereço; sem ele, cada chamada abriria a sua janela
eval "$(dbus-daemon --session --fork --print-address=1 | sed 's/^/export DBUS_SESSION_BUS_ADDRESS=/')"
sway -c "$HOME/.config/sway/config" > /tmp/sway.log 2>&1 &
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
# Sem um teclado vivo, cada wtype cria o seu, e a primeira tecla se perde (o Esc sozinho, o f de "foot")
wtype -s 3600000 -k F24 & keeper=$!
trap 'kill $keeper 2>/dev/null' EXIT
sleep 0.5

failed=0
ok() { printf 'ok      %s\n' "$1"; }
fail() { printf 'FALHOU  %s\n' "$1"; failed=1; }
shot() { grim "/out/help-$1.png" 2>/dev/null; }
windows() { swaymsg -t get_tree | grep -c '"app_id": "org.swaydesktop.Help"'; }
wait_windows() {  # quantas janelas esperar
  for _ in $(seq $ticks); do [ "$(windows)" = "$1" ] && return 0; sleep 0.1; done
  return 1
}
key() { timeout 5 wtype "$@"; sleep 0.4; }
export G_DEBUG=fatal-criticals

if [ "$mode" = repo ]; then
  swaymsg 'exec ~/.local/bin/sway-help >> /tmp/app.log 2>&1' > /dev/null
  wait_windows 1 || { fail "a janela não abriu"; cat /tmp/app.log; exit 1; }
  swaymsg '[app_id="org.swaydesktop.Help"] resize set 1200 1900, move position 0 0' > /dev/null
  sleep 1.5; shot repo-1-atalhos
  key -M ctrl -k Tab -m ctrl; sleep 1; shot repo-2-cartoes
  grep -q "CRITICAL\|panicked" /tmp/app.log && { fail "avisos no log"; cat /tmp/app.log; }
  exit $failed
fi

# O cartão da Central (o modo "settings", e também o fim da rodada padrão, a do run-all-tests)
settings_cards() {
  center=swaydesk-$(printf settings)
  for case in sem com; do
    if [ "$case" = com ]; then
      mkdir -p /tmp/center "$HOME/.local/share/applications"
      printf '#!/bin/sh\necho "$#:$*" >/tmp/center/args\n' > /tmp/center/$center && chmod +x /tmp/center/$center
      cp /repo/packaging/files/org.swaydesktop.Settings.desktop "$HOME/.local/share/applications/"
      run="env PATH=/tmp/center:$PATH"
    else
      run=env
    fi
    swaymsg "exec $run ~/.local/bin/sway-help >> /tmp/app.log 2>&1" > /dev/null
    wait_windows 1 || { fail "a janela não abriu ($case)"; cat /tmp/app.log; exit 1; }
    swaymsg '[app_id="org.swaydesktop.Help"] resize set 1200 700, move position 0 0' > /dev/null
    sleep 1; key -M ctrl -k Tab -m ctrl; sleep 1; shot "settings-$case"
    # A busca pelo nome: só o cartão da Central (com) ou nenhum (sem)
    key Settings; sleep 0.5; shot "settings-$case-busca"
    # Onde fica o ícone do primeiro resultado: com a Central, o cartão dela (várias cores); sem, só o fundo
    n=$(grim -t ppm -g "40,105 30x30" - | python3 -c 'import sys
data = sys.stdin.buffer.read().split(b"\n", 3)[3]
print(len({data[i:i + 3] for i in range(0, len(data), 3)}))')
    if [ "$case" = com ]; then want=varias; else want=1; fi
    got=$([ "$n" -gt 1 ] && echo varias || echo 1)
    [ "$got" = "$want" ] && ok "a busca por Settings $case a Central: $want cor(es) no 1º resultado" \
      || fail "a busca por Settings $case a Central: $got cor(es) no 1º resultado, esperado $want (capturas help-settings-$case*.png)"
    key -k Escape; key -k Escape; wait_windows 0 || fail "não fechou ($case)"
  done
}
if [ "$mode" = settings ]; then
  settings_cards
  grep -q "CRITICAL\|panicked" /tmp/app.log && { fail "avisos no log"; cat /tmp/app.log; }
  exit $failed
fi

if [ "$mode" = profiles ]; then
  # Sem ponteiro no headless, o seletor não é clicado aqui (a troca, a confirmação do dwm e o Cancelar são
  # da VM): só a abertura com cada perfil em uso. O nome no seletor e o aviso do dwm vêm do config.toml
  conf=$HOME/.config/swaydesk/config.toml
  for profile in dwm i3; do
    printf 'accent = "#199ca8"\nshortcuts = "%s"\n' "$profile" > "$conf"
    swaymsg 'exec ~/.local/bin/sway-help >> /tmp/app.log 2>&1' > /dev/null
    wait_windows 1 || { fail "a janela não abriu ($profile)"; cat /tmp/app.log; exit 1; }
    sleep 1.5; shot "profiles-$profile"
    ok "abre com o perfil $profile (captura help-profiles-$profile.png)"
    key -k Escape; wait_windows 0 || fail "não fechou"
  done
  # Os dotfiles: o config de verdade e os keys/ num "repositório", o ~/.config/sway/config um link para ele
  mkdir -p /tmp/repo-dots/sway && mv "$HOME/.config/sway/keys" /tmp/repo-dots/sway/keys
  cp "$HOME/.config/sway/config" /tmp/repo-dots/sway/config && ln -sf /tmp/repo-dots/sway/config "$HOME/.config/sway/config"
  swaymsg 'exec ~/.local/bin/sway-help >> /tmp/app.log 2>&1' > /dev/null
  wait_windows 1 || { fail "a janela não abriu (dotfiles)"; cat /tmp/app.log; exit 1; }
  sleep 1.5; shot profiles-dotfiles
  ok "abre com o config como link (captura help-profiles-dotfiles.png: o seletor deve aparecer)"
  key -k Escape; wait_windows 0 || fail "não fechou"
  grep -q "CRITICAL\|panicked" /tmp/app.log && { fail "avisos no log"; cat /tmp/app.log; }
  exit $failed
fi

# O atalho do config abre (pelo lançador sway-help, como na sessão)
swaymsg 'exec env G_DEBUG=fatal-criticals ~/.local/bin/sway-help >> /tmp/app.log 2>&1' > /dev/null
if wait_windows 1; then ok "o lançador abre a janela (app_id org.swaydesktop.Help)"; else fail "a janela não abriu"; cat /tmp/app.log; exit 1; fi
sleep 1
shot 1-atalhos
pid=$(pgrep -n -x swaydesk-help || pgrep -n -x sway-help)
exe=$(readlink "/proc/$pid/exe")
case $exe in *swaydesk-help) ok "roda o binário em Rust" ;; *) fail "binário errado: $exe" ;; esac

timeout 10 "$HOME/.local/bin/sway-help" >> /tmp/app.log 2>&1 || fail "abrir de novo não voltou em 10 s"
wait_windows 0 && ok "abrir de novo fecha" || fail "abrir de novo deixou $(windows) janelas"

"$HOME/.local/bin/sway-help" >> /tmp/app.log 2>&1 &
wait_windows 1 || fail "não reabriu"
sleep 1
key foot
shot 2-busca-foot
key -k Escape
[ "$(windows)" = 1 ] && ok "o Esc com texto só limpa a busca" || fail "o Esc com texto fechou a janela"
shot 3-busca-limpa
# A aba "Using Swaydesk" pelo Ctrl+Tab
key -M ctrl -k Tab -m ctrl
sleep 0.5
shot 4-cartoes
key -k Escape
wait_windows 0 && ok "o Esc com a busca vazia fecha" || fail "o Esc com a busca vazia não fechou"
settings_cards

grep -q "CRITICAL\|panicked" /tmp/app.log && { fail "avisos no log"; cat /tmp/app.log; }
exit $failed
