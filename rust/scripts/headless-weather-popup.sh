#!/bin/bash
# Rodada curta: o contêiner sai (--rm) ao fim do teste, e nada dele fica no ar. O host vê os processos dele, com o
# seu UID (o pgrep -u da sessão também os acha): uma grade, uma dock ou um lock esquecido confunde a sua sessão.
# Testa o popup do tempo num Sway headless, sem rede: um cache fresco do wttr.in montado aqui (o mesmo
# arquivo que o ícone e o popup dividem), e a captura (grim) de cada versão. Roda dentro do contêiner
# swaydesk-smoke:debian13, como o headless-toasts.sh, com o swaydesk-weather-popup e o swaydesk-weather em
# /bin. Confere também que o ícone em Rust e o em Python dão a mesma linha do mesmo cache. Sai com 1 se
# algo falhar.
set -u
. /repo/rust/scripts/lib-wait.sh
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$HOME/.local/bin" "$HOME/.config/swaydesk" && mkdir -p -m 700 "$XDG_RUNTIME_DIR"
for f in /repo/home/.local/bin/*; do [ -f "$f" ] && ln -sf "$f" "$HOME/.local/bin/"; done
for f in /bin/swaydesk-*; do [ "${f##*/}" = swaydesk-vpointer ] || cp "$f" "$HOME/.local/bin/"; done
printf 'weather_location = "Teste"\n' > "$HOME/.config/swaydesk/config.toml"
python3 - <<'PY'
import json, time
from datetime import date, timedelta
today = date.today()
def hours(code, base):
    return [{"time": str(h * 300), "chanceofrain": str(10 * (h % 4)), "weatherCode": code, "tempC": str(base + h),
             "weatherDesc": [{"value": "Partly cloudy"}]} for h in range(8)]
data = {
    "_fetched": time.time(), "_place": "Cidade de Teste, BR",
    "current_condition": [{"weatherCode": "116", "temp_C": "27", "FeelsLikeC": "29", "humidity": "70",
                           "windspeedKmph": "18", "winddir16Point": "ESE", "weatherDesc": [{"value": "Partly cloudy"}]}],
    "weather": [{"date": (today + timedelta(days=i)).isoformat(), "mintempC": str(22 + i), "maxtempC": str(30 + i),
                 "astronomy": [{"sunrise": "05:01 AM", "sunset": "05:30 PM"}],
                 "hourly": hours(["116", "176", "113"][i], 22 + i)} for i in range(3)],
}
json.dump(data, open("/tmp/xdg/waybar-weather.json", "w"))
PY
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 LANG=C.UTF-8
printf 'output HEADLESS-1 resolution 1280x720\n' > /tmp/sway.conf
sway -c /tmp/sway.conf > /tmp/sway.log 2>&1 &
wait_sway
export WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.*.sock)
failed=0
expect() {
  if [ "$2" = "$3" ]; then printf 'ok      %s\n' "$1"; else printf 'FALHOU  %s: obtido "%s", esperado "%s"\n' "$1" "$3" "$2"; failed=1; fi
}
rust_bar=$(SWAY_WEATHER_IMPL=rust "$HOME/.local/bin/waybar-weather-bar")
python_bar=$(SWAY_WEATHER_IMPL=python "$HOME/.local/bin/waybar-weather-bar")
expect "o ícone em Rust dá a linha do Python" "$python_bar" "$rust_bar"
for impl in rust python; do
  SWAY_WEATHER_POPUP_IMPL=$impl "$HOME/.local/bin/waybar-weather" 2>>/tmp/popup.err & popup=$!
  sleep 2
  echo "$impl: popup $(readlink /proc/$popup/exe)"
  grim "/out/weather-$impl.png"
  expect "$impl: o popup abre" "aberto" "$(kill -0 "$popup" 2>/dev/null && echo aberto || echo fechado)"
  kill $popup 2>/dev/null; wait $popup 2>/dev/null; sleep 0.5
done
grep -v '^$' /tmp/popup.err | grep -v 'session bus' | head -10
exit "$failed"
