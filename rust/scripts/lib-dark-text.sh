# shellcheck shell=bash
# Para os testes headless das superfícies sempre escuras (as listas dos popups, a grade, o Alt+Tab): um adw-gtk3-dark
# falso, como o 6.5 num GTK antes do 4.20, em que o CSS escuro só vem dentro de `@media (prefers-color-scheme: dark)`
# e o gtk-dark.css é o mesmo arquivo. A cor do texto fora do @media é magenta: um rótulo ou ícone simbólico sem cor
# própria que pegue o CSS claro do tema aparece magenta no print (no adw-gtk3 de verdade, o texto escuro do tema
# claro nos cartões escuros: o título da linha não selecionada da área de transferência no vídeo). Com a troca pelo
# Adwaita-dark embutido (swaydesk_ui::always_dark), nada sai magenta. O GTK 3 (a versão em Python) não lê o gtk-4.0
# daqui. Uso: source, fake_adw_gtk3_dark antes de abrir o componente, no_light_text NOME depois de abri-lo.

fake_adw_gtk3_dark() {
  local dir=$HOME/.themes/adw-gtk3-dark/gtk-4.0
  mkdir -p "$dir" "${XDG_CONFIG_HOME:-$HOME/.config}/gtk-4.0"
  cat > "$dir/gtk.css" <<'CSS'
* { color: #ff00ff; }
window, window.background, .background { background-color: #fafafb; color: #ff00ff; }
@media (prefers-color-scheme: dark) {
  * { color: #ffffff; }
  window, window.background, .background { background-color: #1e1e1f; color: #ffffff; }
}
CSS
  cp "$dir/gtk.css" "$dir/gtk-dark.css"
  printf '[Settings]\ngtk-theme-name=adw-gtk3-dark\ngtk-application-prefer-dark-theme=1\n' \
    > "${XDG_CONFIG_HOME:-$HOME/.config}/gtk-4.0/settings.ini"
}

# no_light_text NOME: o print da tela agora não tem nenhum pixel na cor do texto do tema claro (magenta forte; o
# antisserrilhado nas bordas mistura com o fundo e fica abaixo do limite). Imprime ok/FALHOU e põe failed=1.
no_light_text() {
  local n
  # O cabeçalho do PPM termina num único espaço depois do maxval: um split() comeria também o primeiro byte dos
  # pixels quando ele parece um espaço (um fundo #202020 no canto)
  n=$(grim -t ppm - | python3 -c '
import re, sys
data = sys.stdin.buffer.read()
m = re.match(rb"P6\s+(\d+)\s+(\d+)\s+(\d+)\s", data)
w, h, p = int(m[1]), int(m[2]), data[m.end():]
print(sum(1 for i in range(0, w * h * 3, 3) if p[i] > 150 and p[i + 1] < 100 and p[i + 2] > 150))' 2>/dev/null)
  if [ -z "$n" ]; then
    printf 'FALHOU  %s: sem print da tela (grim)\n' "$1"
    failed=1
  elif [ "$n" = 0 ]; then
    printf 'ok      %s: nenhum texto na cor do tema claro\n' "$1"
  else
    printf 'FALHOU  %s: %s pixels na cor do texto do tema claro (adw-gtk3-dark sem a troca pelo Adwaita-dark)\n' "$1" "$n"
    failed=1
  fi
}
