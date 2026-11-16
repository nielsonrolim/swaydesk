#!/bin/bash
# Confere cada perfil de atalhos (home/.config/sway/keys) com o próprio Sway: o config dos dotfiles com o
# config.d/00-keys.conf de cada perfil passa no sway -C sem erro nem aviso (uma tecla repetida sem --no-warn, um
# unbindsym de tecla que não existe, um nome de tecla errado). Roda dentro do contêiner swaydesk-smoke:debian13,
# com o repositório em /repo:
#
#   docker run --rm -u $(id -u):$(id -g) -v /etc/passwd:/etc/passwd:ro -v $PWD:/repo:ro \
#     swaydesk-smoke:debian13 timeout 120 /repo/rust/scripts/headless-profiles.sh
#
# Sai com 1 se algum perfil falhar.
set -u
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1
mkdir -p "$HOME/.config/sway/config.d" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
# O wallpaper que o config usa (o bing-wallpaper o baixa na sessão)
mkdir -p "$HOME/.local/share/wallpaper" && : > "$HOME/.local/share/wallpaper/current.jpg"
failed=0
for profile in swaydesk i3 hyprland niri dwm; do
  printf 'include "/repo/home/.config/sway/keys/base.conf"\ninclude "/repo/home/.config/sway/keys/%s.conf"\n' "$profile" \
    > "$HOME/.config/sway/config.d/00-keys.conf"
  # -d: o aviso de uma tecla que troca outra ("Overwriting binding") só sai no modo de depuração
  out=$(sway -d -C -c /repo/home/.config/sway/config 2>&1); rc=$?
  # O que o perfil troca da base de propósito (com --no-warn): só o dwm, o Alt+Space e o Alt+Tab
  allowed='^$'
  [ "$profile" = dwm ] && allowed="Overwriting binding 'Mod1\+(space|Tab)'"
  warns=$(grep -E '\[sway/' <<<"$out" | grep -iE 'overwrit|unable|error|invalid|unknown (command|key|option)|no binding' \
    | grep -vE "$allowed|Config stored for output" || true)
  if [ $rc = 0 ] && [ -z "$warns" ]; then
    printf 'ok      %s\n' "$profile"
  else
    printf 'FALHOU  %s (rc %s)\n%s\n' "$profile" "$rc" "$warns"
    failed=1
  fi
done
exit $failed
