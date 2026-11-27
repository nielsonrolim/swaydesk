#!/usr/bin/env bash
# Instala os arquivos de um pacote binário numa árvore (DESTDIR), a partir dos binários já compilados.
# O debian/rules e o PKGBUILD chamam este script, para os dois formatos levarem os mesmos arquivos.
# uso: packaging/install-tree.sh PACOTE DESTDIR [PASTA_DOS_BINÁRIOS]
#      packaging/install-tree.sh --crates    os crates a compilar (-p swaydesk-...), de todos os pacotes
#      packaging/install-tree.sh --sources   os arquivos de fora de rust/ e packaging/ que este script lê,
#                                            relativos à raiz: o tarball de origem (make-orig.sh) só leva
#                                            esses (o tests/test_packaging.py confere que bastam)
set -euo pipefail
root=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
list=$root/packaging/components.list

components() {  # os componentes de um pacote; vazio se ele não tem binário
  awk -v pkg="$1:" '$1 == pkg { $1 = ""; print }' "$list"
}

if [[ ${1:-} == --crates ]]; then
  for name in $(awk '!/^#/ && NF { $1 = ""; print }' "$list"); do printf -- '-p swaydesk-%s ' "$name"; done
  echo
  exit 0
fi
data=$root/home/.local/share
bin=$root/home/.local/bin
# Os scripts auxiliares: o Debian os põe em /usr/libexec, e o Arch em /usr/lib (SWAYDESK_LIBEXEC=/usr/lib)
libexec=${SWAYDESK_LIBEXEC:-/usr/libexec}/swaydesk
# Os scripts auxiliares que os binários e as configs chamam (paths::helper), cada um no pacote do componente que o
# chama (desde a 0.1.7; antes, todos no swaydesk-scripts), como pacote:script. Os que mais de um pacote chama
# ficam no swaydesk-common (o waybar-notify dos popups, o session-action do menu e do gerenciador de sessão). O
# sway-idle vai com a cafeína, que só funciona por ele (ela troca o swayidle em execução). O ícone em bash de cada
# item da barra (waybar-volume…) vai com o popup dele, e o sway-osd (as teclas de volume e brilho) com o indicador,
# junto com o sway-brightness que ele roda. O tests/test_packaging.py confere que cada um vai num pacote só
SCRIPTS=(swaydesk-common:session-action swaydesk-common:waybar-notify swaydesk-common:sway-terminal
         swaydesk-drawer:sway-files
         swaydesk-brightness:sway-nightlight swaydesk-brightness:waybar-brightness
         swaydesk-osd:sway-osd swaydesk-osd:sway-brightness
         swaydesk-calendar:waybar-clock swaydesk-media:waybar-media swaydesk-audio:waybar-volume
         swaydesk-battery:waybar-battery swaydesk-caffeine:waybar-caffeine
         swaydesk-caffeine:sway-caffeine
         swaydesk-capture:sway-shot swaydesk-capture:sway-rec swaydesk-capture:sway-notify-open
         swaydesk-lock:sway-lock swaydesk-lock:sway-lock-watch swaydesk-lock:sway-gtklock
         swaydesk-caffeine:sway-idle swaydesk-caffeine:sway-idle-suspend)
# Sem o bit de execução (o debian/rules os tira do dh_fixperms), no swaydesk-common: o calendário e o bloqueio em
# Python os importam, e os scripts em shell incluem o sway-i18n.sh e o waybar-icon-lib (os ícones da barra) com ".". Só a biblioteca padrão: o common não
# depende do python3, e quem os roda (o swaydesk-calendar, o swaydesk-lock do Debian) traz o python3
MODULES=(sway_i18n.py desktop_config.py sway-i18n.sh waybar-icon-lib)
# O calendário, no swaydesk-calendar, na mesma pasta: ele roda o waybar_holidays.py com o python3, que importa os
# dois módulos acima da própria pasta
CALENDAR_MODULES=(waybar_holidays.py)
# O bloqueio em Python, no swaydesk-lock do Debian e do Ubuntu (o waybar_media, para a mídia na tela)
PYTHON_LOCK=(sway-lock-gtk sway_lock.py sway_pam.py waybar_media.py)
# O swaydesk-desktop: os scripts que o config do Sway e o da Waybar do pacote chamam, além dos auxiliares, e a sessão (o sway-session e o sway-gpu, que no --system vão para /usr/local/bin). O
# tests/test_packaging.py confere que todo programa que as configs chamam está num pacote
SESSION_SCRIPTS=(sway-after-env sway-if-enabled sway-waybar sway-env-cleanup sway-polkit-agent sway-update-notifier
                 sway-language-packs
                 blueman-agent bing-wallpaper sway-super sway-resize sway-master sway-ws sway-launcher
                 sway-portal-gtk sway-has-display sway-updates waybar-updates sway-autostart
                 sway-input-method)
share=/usr/share/swaydesk-desktop

rust_components() {  # os componentes em Rust de todos os pacotes
  awk '!/^#/ && NF { $1 = ""; print }' "$list"
}

# Os lançadores de home/.local/bin que só chamam um binário em Rust: o nome do arquivo e o componente, um par
# por linha (packaging/launchers.list; o tarball não leva os lançadores em Python)
rust_launchers() {
  awk '!/^#/ && NF' "$root/packaging/launchers.list"
}

# Os arquivos de dados que os pacotes levam de home/ e system/ (os scripts estão nas listas acima)
CAPTURE_ENTRIES=(gradia-screenshot.desktop gradia-screen-record.desktop)
THEME_ENTRY=org.swaydesktop.Theme.desktop
DISPLAYS_ENTRY=org.swaydesktop.Displays.desktop
HELP_ENTRY=org.swaydesktop.Help.desktop
# Os ícones de Captura de Tela, Gravar Tela e do app Tema: cópias de ícones do Papirus (GPL-3, ver o
# debian/copyright), no hicolor para aparecerem com qualquer tema de ícones, porque o papirus-icon-theme é só
# recomendado e o Adwaita não tem o preferences-desktop-theme. E os da bandeja que o Papirus não tem (o
# unattended-upgrades do Ubuntu pede o system-software-update-symbolic): no hicolor, a reserva de todo tema.
# Caminhos dentro de hicolor/scalable
HICOLOR_ICONS=(apps/swaydesk-screenshot.svg apps/swaydesk-screen-record.svg apps/org.swaydesktop.Theme.svg
  status/system-software-update-symbolic.svg)
SESSION_CONFIGS=(sway/config waybar/config.jsonc waybar/style.css xdg-desktop-portal/sway-portals.conf)  # de home/.config
# Os atalhos: o base.conf e um arquivo por perfil do shortcuts (sway/keys, ao lado do sway/config)
KEY_PROFILES=(base swaydesk i3 hyprland niri dwm)
SYSTEM_SCRIPTS=(sway-session sway-gpu)  # de system/usr/local/bin
# O autostart do XDG (sway-autostart): o target e os drop-ins que pulam, na sessão do Swaydesk, as entradas que
# duplicam o que ele já roda. De home/.config/systemd/user, onde os dotfiles os ligam
AUTOSTART_TARGET=swaydesk-autostart.target
autostart_dropins() { (cd "$root/home/.config/systemd/user" && printf '%s\n' app-*@autostart.service.d/swaydesk.conf); }

if [[ ${1:-} == --sources ]]; then
  for name in "${SCRIPTS[@]#*:}" "${MODULES[@]}" "${CALENDAR_MODULES[@]}" "${SESSION_SCRIPTS[@]}" "${PYTHON_LOCK[@]}"; do
    echo "home/.local/bin/$name"
  done
  (cd "$root" && printf '%s\n' home/.local/share/locale/*/LC_MESSAGES/sway-desktop.mo)
  for entry in "${CAPTURE_ENTRIES[@]}" "$THEME_ENTRY" "$HELP_ENTRY" "$DISPLAYS_ENTRY"; do echo "home/.local/share/applications/$entry"; done
  for icon in "${HICOLOR_ICONS[@]}"; do echo "home/.local/share/icons/hicolor/scalable/$icon"; done
  (cd "$root" && printf '%s\n' home/.local/share/sway-desktop/templates/* home/.local/share/sway-desktop/dconf/*)
  for file in "${SESSION_CONFIGS[@]}"; do echo "home/.config/$file"; done
  for name in "${KEY_PROFILES[@]}"; do echo "home/.config/sway/keys/$name.conf"; done
  (cd "$root" && find home-copy -type f)
  for name in "${SYSTEM_SCRIPTS[@]}"; do echo "system/usr/local/bin/$name"; done
  echo "home/.config/systemd/user/$AUTOSTART_TARGET"
  autostart_dropins | sed 's|^|home/.config/systemd/user/|'
  exit 0
fi

pkg=${1:?pacote} dest=${2:?DESTDIR} bins=${3:-$root/rust/target/release}
if [[ $pkg != swaydesk-common ]] && ! grep -q "^$pkg:" "$list"; then
  echo "install-tree: unknown package: $pkg" >&2
  exit 2
fi
# Cada binário com a sua man page (packaging/man, uma por binário: o tests/test_man_pages.py confere). Sem
# compactar: o dh_compress do Debian e o makepkg do Arch compactam o que está em /usr/share/man
man=$root/packaging/man
for name in $(components "$pkg"); do
  install -Dm755 "$bins/swaydesk-$name" "$dest/usr/bin/swaydesk-$name"
  install -Dm644 "$man/swaydesk-$name.1" "$dest/usr/share/man/man1/swaydesk-$name.1"
done
# Os serviços de usuário dos componentes residentes (packaging/units.list): com o binário, em qualquer pacote
# que a divisão o ponha. Para uma sessão que não é a do Swaydesk; nenhum pacote os liga
for name in $(components "$pkg"); do
  awk -v c="$name" '!/^#/ && $1 == c { print $2 }' "$root/packaging/units.list" | while read -r unit; do
    install -Dm644 "$root/packaging/files/units/$unit" "$dest/usr/lib/systemd/user/$unit"
  done
done

# Os scripts auxiliares do pacote (o SCRIPTS acima)
for entry in "${SCRIPTS[@]}"; do
  [[ ${entry%%:*} == "$pkg" ]] && install -Dm755 "$bin/${entry#*:}" "$dest$libexec/${entry#*:}"
done
# Os nomes que as configs e os scripts usam (sway-dock, waybar-audio…) apontam para o binário em Rust, como no
# install.sh --rust-only (o lançador em Python de cada um diz o componente, sway_rust.exec_rust). Cada link vai
# no pacote do binário: os ícones da barra e o sway-osd reconhecem o popup e o indicador por esse nome (desde a
# 0.1.7; antes, todos no swaydesk-desktop)
mine=" $(components "$pkg") "
rust_launchers | while read -r name comp; do
  if [[ $mine == *" $comp "* ]]; then
    install -d "$dest$libexec"
    ln -sfn "/usr/bin/swaydesk-$comp" "$dest$libexec/$name"
  fi
done

# Nos arquivos de config do pacote, os scripts e os lançadores de ~/.local/bin passam a ser os do libexec
package_paths() {
  sed -e "s|^set \$bin  *~/\.local/bin\$|set \$bin $libexec|" -e "s|~/\.local/bin/|$libexec/|g" "$1"
}
# Os trechos opcionais para quem usa o Sway puro e a própria Waybar (desde a 0.1.7, packaging/snippets): o de cada
# pacote leva o nome dele. Nada os lê sozinho: o usuário inclui o do Sway no config dele e cola o da Waybar no dele
# (o /etc/sway/config.d, que o config padrão do Sway inclui, ligaria para todos). Como nas configs da sessão, os
# programas de ~/.local/bin passam a ser os do libexec: os lançadores do próprio pacote (o nome diz o papel)
snippets=$root/packaging/snippets
if [[ -f $snippets/sway/50-$pkg.conf ]]; then
  install -d "$dest/usr/share/swaydesk/sway/config.d"
  package_paths "$snippets/sway/50-$pkg.conf" > "$dest/usr/share/swaydesk/sway/config.d/50-$pkg.conf"
  chmod 644 "$dest/usr/share/swaydesk/sway/config.d/50-$pkg.conf"
fi
if [[ -f $snippets/waybar/$pkg.jsonc ]]; then
  install -d "$dest/usr/share/swaydesk/waybar"
  package_paths "$snippets/waybar/$pkg.jsonc" > "$dest/usr/share/swaydesk/waybar/$pkg.jsonc"
  chmod 644 "$dest/usr/share/swaydesk/waybar/$pkg.jsonc"
fi

case $pkg in
  swaydesk-common)
    # Os ícones da Symbols Nerd Font (a mesma v3.4.0 que o install.sh baixa), que o Debian não empacota. O
    # Arch tem o ttf-nerd-fonts-symbols no repositório: lá, SWAYDESK_BUNDLED_FONT=0 e o pacote depende dele
    if [[ ${SWAYDESK_BUNDLED_FONT:-1} == 1 ]]; then
      install -Dm644 "$root/packaging/fonts/SymbolsNerdFont-Regular.ttf" \
        "$dest/usr/share/fonts/truetype/swaydesk/SymbolsNerdFont-Regular.ttf"
    fi
    for name in "${MODULES[@]}"; do
      install -Dm644 "$bin/$name" "$dest$libexec/$name"
    done
    # O que os trechos do Sway puro chamam para subir a unidade de cada componente (packaging/snippets)
    install -Dm755 "$root/packaging/snippets/swaydesk-sway-unit" "$dest$libexec/swaydesk-sway-unit"
    # Os ícones do hicolor, aqui porque a Ajuda (swaydesk-help) também os mostra
    for icon in "${HICOLOR_ICONS[@]}"; do
      install -Dm644 "$data/icons/hicolor/scalable/$icon" "$dest/usr/share/icons/hicolor/scalable/$icon"
    done
    # O destaque de cada conta no AccountsService (o swaydesk-desktop-config grava, a tela de login lê): a
    # interface no lugar das do D-Bus e o link na pasta que o accounts-daemon lê ao subir
    install -Dm644 "$root/packaging/files/org.swaydesktop.Accounts.xml" "$dest/usr/share/dbus-1/interfaces/org.swaydesktop.Accounts.xml"
    install -d "$dest/usr/share/accountsservice/interfaces"
    ln -sfn ../../dbus-1/interfaces/org.swaydesktop.Accounts.xml "$dest/usr/share/accountsservice/interfaces/org.swaydesktop.Accounts.xml"
    # A visão geral, swaydesk(7): o pacote de que todos os outros dependem
    install -Dm644 "$man/swaydesk.7" "$dest/usr/share/man/man7/swaydesk.7"
    # As traduções: os binários procuram em bin/../share/locale, que aqui é /usr/share/locale
    for mo in "$data"/locale/*/LC_MESSAGES/sway-desktop.mo; do
      lang=${mo#"$data/locale/"}
      install -Dm644 "$mo" "$dest/usr/share/locale/$lang"
    done
    ;;
  swaydesk-capture)
    # Captura de Tela e Gravar Tela na grade, no lançador e no dock, como nos dotfiles. São entradas nossas: o
    # prefixo gradia- é só do nome do arquivo, e o .desktop do Gradia é o be.alexandervanhee.gradia.desktop.
    # O Exec dos dotfiles é o lançador de ~/.local/bin, que o pacote não tem: aqui, o binário em /usr/bin
    install -d "$dest/usr/share/applications"
    for entry in "${CAPTURE_ENTRIES[@]}"; do
      sed 's/^Exec=sway-capture /Exec=swaydesk-capture /' "$data/applications/$entry" > "$dest/usr/share/applications/$entry"
      chmod 644 "$dest/usr/share/applications/$entry"
      grep -q '^Exec=swaydesk-capture ' "$dest/usr/share/applications/$entry" || {
        echo "install-tree: $entry: no Exec=sway-capture line to point at the package binary" >&2
        exit 1
      }
    done
    ;;
  swaydesk-calendar)
    for name in "${CALENDAR_MODULES[@]}"; do
      install -Dm644 "$bin/$name" "$dest$libexec/$name"
    done
    ;;
  swaydesk-help)
    # A Ajuda do Swaydesk na grade. O nome do arquivo é o app_id do app (a dock acha o ícone por ele), e o
    # Exec dos dotfiles é o lançador de ~/.local/bin, que o pacote não tem: aqui, o binário em /usr/bin
    entry=$HELP_ENTRY
    install -d "$dest/usr/share/applications"
    sed 's/^Exec=sway-help$/Exec=swaydesk-help/' "$data/applications/$entry" > "$dest/usr/share/applications/$entry"
    chmod 644 "$dest/usr/share/applications/$entry"
    grep -qx 'Exec=swaydesk-help' "$dest/usr/share/applications/$entry" || {
      echo "install-tree: $entry: no Exec=sway-help line to point at the package binary" >&2
      exit 1
    }
    ;;
  swaydesk-theme-tool)
    # Os modelos e os perfis do dconf ficam em bin/../share/sway-desktop, onde o share_dir os acha
    for file in "$data"/sway-desktop/templates/* "$data"/sway-desktop/dconf/*; do
      install -Dm644 "$file" "$dest/usr/share/sway-desktop/${file#"$data/sway-desktop/"}"
    done
    ;;
  swaydesk-portal)
    install -Dm644 "$root/packaging/files/org.freedesktop.impl.portal.desktop.swaydesktop.service" \
      "$dest/usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.swaydesktop.service"
    install -Dm644 "$root/packaging/files/swaydesktop.portal" "$dest/usr/share/xdg-desktop-portal/portals/swaydesktop.portal"
    # O portals.conf da sessão como trecho opcional para o Sway puro: nada o lê até o usuário copiá-lo para o
    # ~/.config/xdg-desktop-portal (um em /etc/xdg ou /usr/share/xdg-desktop-portal mudaria o Sway de todos)
    install -Dm644 "$root/packaging/snippets/xdg-desktop-portal/sway-portals.conf" \
      "$dest/usr/share/swaydesk/xdg-desktop-portal/sway-portals.conf"
    ;;
  swaydesk-theme-app)
    # O app Tema do Desktop no menu. O nome do arquivo é o app_id do app, que é como a dock acha o ícone. O
    # Exec dos dotfiles é o lançador de ~/.local/bin, que o pacote não tem: aqui, o binário em /usr/bin
    entry=$THEME_ENTRY
    install -d "$dest/usr/share/applications"
    sed 's/^Exec=sway-theme-app\( \|$\)/Exec=swaydesk-theme-app\1/' "$data/applications/$entry" > "$dest/usr/share/applications/$entry"
    chmod 644 "$dest/usr/share/applications/$entry"
    grep -qx 'Exec=swaydesk-theme-app --settings-panel' "$dest/usr/share/applications/$entry" || {
      echo "install-tree: $entry: no Exec=sway-theme-app --settings-panel line to point at the package binary" >&2
      exit 1
    }
    ;;
  swaydesk-displays)
    # O app Telas, do mesmo jeito (o nome do arquivo é o app_id, e o swaydesk-help o abre por ele)
    entry=$DISPLAYS_ENTRY
    install -d "$dest/usr/share/applications"
    sed 's/^Exec=sway-displays\( \|$\)/Exec=swaydesk-displays\1/' "$data/applications/$entry" > "$dest/usr/share/applications/$entry"
    chmod 644 "$dest/usr/share/applications/$entry"
    grep -qx 'Exec=swaydesk-displays --settings-panel' "$dest/usr/share/applications/$entry" || {
      echo "install-tree: $entry: no Exec=sway-displays --settings-panel line to point at the package binary" >&2
      exit 1
    }
    ;;
  swaydesk-settings)
    # A Central de Configurações (0.2.0): só existe nos pacotes, e o .desktop (nome = app_id, uma ação por
    # painel) fica no packaging/
    install -Dm644 "$root/packaging/files/org.swaydesktop.Settings.desktop" \
      "$dest/usr/share/applications/org.swaydesktop.Settings.desktop"
    # Um .desktop por painel (a grade e o rofi acham o painel pelo nome): só neste pacote, com o TryExec da Central
    for entry in "$root"/packaging/files/settings-panels/*.desktop; do
      install -Dm644 "$entry" "$dest/usr/share/applications/${entry##*/}"
    done
    ;;
  swaydesk-desktop)
    # A sessão "Sway Desktop": só com os pacotes, sem os dotfiles. O sway-session --package abre o Sway com
    # o config daqui, e o setup copia o skel (o home-copy/) para a HOME no primeiro login
    for name in "${SESSION_SCRIPTS[@]}"; do
      install -Dm755 "$bin/$name" "$dest$libexec/$name"
    done
    for name in "${SYSTEM_SCRIPTS[@]}"; do
      install -Dm755 "$root/system/usr/local/bin/$name" "$dest$libexec/$name"
    done
    mkdir -p "$dest$share/sway" "$dest$share/waybar"
    package_paths "$root/home/.config/sway/config" > "$dest$share/sway/config"
    mkdir -p "$dest$share/sway/keys"
    for name in "${KEY_PROFILES[@]}"; do
      package_paths "$root/home/.config/sway/keys/$name.conf" > "$dest$share/sway/keys/$name.conf"
      chmod 644 "$dest$share/sway/keys/$name.conf"
    done
    package_paths "$root/home/.config/waybar/config.jsonc" > "$dest$share/waybar/config.jsonc"
    # As cores e as fontes vêm do embrulho por usuário que o generate escreve; aqui, só as regras
    grep -v '^@import' "$root/home/.config/waybar/style.css" > "$dest$share/waybar/style.css"
    chmod 644 "$dest$share/sway/config" "$dest$share/waybar/config.jsonc" "$dest$share/waybar/style.css"
    (cd "$root/home-copy" && find . -type f) | while IFS= read -r file; do
      install -Dm644 "$root/home-copy/$file" "$dest$share/skel/${file#./}"
    done
    install -d "$dest/usr/share/wayland-sessions"
    sed "s|@LIBEXEC@|$libexec|" "$root/packaging/files/swaydesk.desktop" > "$dest/usr/share/wayland-sessions/swaydesk.desktop"
    chmod 644 "$dest/usr/share/wayland-sessions/swaydesk.desktop"
    # A Waybar e o gammastep dos pacotes sobem em toda sessão que alcança o graphical-session.target; na
    # nossa, a Waybar sobe pelo config e o sway-nightlight chama o gammastep. O --deps os mascara por usuário
    for unit in gammastep.service gammastep-indicator.service; do
      install -Dm644 "$root/packaging/files/not-in-swaydesk.conf" "$dest/usr/lib/systemd/user/$unit.d/swaydesk.conf"
    done
    # A Waybar, além disso, só sobe nos compositores wlroots: o pacote waybar a liga para todos, e no Plasma
    # ela aparecia por cima do painel
    install -Dm644 "$root/packaging/files/waybar-only-on-wlroots.conf" "$dest/usr/lib/systemd/user/waybar.service.d/swaydesk.conf"
    # O autostart do XDG: o target que o sway-autostart inicia e os drop-ins das entradas que o Swaydesk já roda
    install -Dm644 "$root/home/.config/systemd/user/$AUTOSTART_TARGET" "$dest/usr/lib/systemd/user/$AUTOSTART_TARGET"
    autostart_dropins | while IFS= read -r dropin; do
      install -Dm644 "$root/home/.config/systemd/user/$dropin" "$dest/usr/lib/systemd/user/$dropin"
    done
    # Os backends dos portais numa sessão sway (o dos dotfiles fica no ~/.config e vale mais)
    install -Dm644 "$root/home/.config/xdg-desktop-portal/sway-portals.conf" "$dest/etc/xdg/xdg-desktop-portal/sway-portals.conf"
    # O portal do gtk pelo sway-portal-gtk, que só o abre com uma tela viva. Os arquivos dos pacotes do portal
    # ficam intocados
    case ${SWAYDESK_PORTAL_GTK:-dbus} in
      dbus)
        # Debian e Ubuntu (dbus-daemon): a ativação direta, como o .service dos dotfiles em
        # ~/.local/share/dbus-1/services. O .service num diretório do pacote, que o environment.d põe na frente
        # do XDG_DATA_DIRS do systemd do usuário
        dbusdir=$share/dbus
        install -d "$dest$dbusdir/dbus-1/services" "$dest/usr/lib/environment.d"
        sed "s|@LIBEXEC@|$libexec|" "$root/packaging/files/swaydesk-portal-gtk.service" \
          > "$dest$dbusdir/dbus-1/services/org.freedesktop.impl.portal.desktop.gtk.service"
        sed "s|@DIR@|$dbusdir|" "$root/packaging/files/swaydesk-portal-gtk.environment.conf" \
          > "$dest/usr/lib/environment.d/90-swaydesk-portal-gtk.conf"
        chmod 644 "$dest$dbusdir/dbus-1/services/org.freedesktop.impl.portal.desktop.gtk.service" \
          "$dest/usr/lib/environment.d/90-swaydesk-portal-gtk.conf"
        ;;
      systemd)
        # Arch (dbus-broker): um drop-in na unit do portal, sem um segundo .service com o mesmo nome
        install -d "$dest/usr/lib/systemd/user/xdg-desktop-portal-gtk.service.d"
        sed "s|@LIBEXEC@|$libexec|" "$root/packaging/files/swaydesk-portal-gtk.dropin.conf" \
          > "$dest/usr/lib/systemd/user/xdg-desktop-portal-gtk.service.d/swaydesk.conf"
        chmod 644 "$dest/usr/lib/systemd/user/xdg-desktop-portal-gtk.service.d/swaydesk.conf"
        ;;
      *)
        echo "install-tree: SWAYDESK_PORTAL_GTK must be dbus or systemd" >&2
        exit 2
        ;;
    esac
    # O teclado do console que segue o XKB, só no Arch (SWAYDESK_CONSOLE_KEYMAPS, do PKGBUILD): o us(intl) vira
    # us-acentos pela tabela do systemd e do Calamares, sem o AltGr da sessão; com o us-intl.map nesta pasta,
    # os dois o escolhem. O Debian e o Ubuntu montam o do console pelo XKB (console-setup)
    if [ -n "${SWAYDESK_CONSOLE_KEYMAPS:-}" ]; then
      install -Dm644 "$root/packaging/files/console/us-intl.map" "$dest$SWAYDESK_CONSOLE_KEYMAPS/us-intl.map"
    fi
    ;;
  swaydesk-lock)
    # O serviço do PAM do bloqueio (o swaydesk-lock e o sway-lock-gtk): o próprio, sem depender do do swaylock
    install -Dm644 "$root/packaging/files/swaydesk-lock-pam" "$dest/etc/pam.d/swaydesk-lock"
    # O bloqueio em Python (GTK 3 + gtk-session-lock), o segundo do sway-lock: no Debian 13, o gtk4-layer-shell
    # 1.0 não tem o bloqueio, e ele é o bloqueio de fato. Ao lado dos módulos do swaydesk-common
    # (desktop_config, sway_i18n), que ele importa da própria pasta. O Arch não o leva (SWAYDESK_PYTHON_LOCK=0,
    # do PKGBUILD): lá o gtk4-layer-shell tem o bloqueio
    if [[ ${SWAYDESK_PYTHON_LOCK:-1} == 1 ]]; then
      for name in "${PYTHON_LOCK[@]}"; do
        mode=644
        [[ $name == *.py ]] || mode=755
        install -Dm"$mode" "$bin/$name" "$dest$libexec/$name"
      done
    fi
    ;;
  swaydesk-greeter)
    # O greetd abre o Sway da tela pelo drop-in, com o greetd.toml do pacote: o /etc/greetd/config.toml da
    # distro fica intocado. O usuário do greeter e o VT são os do pacote greetd de cada distro: _greetd e 7
    # no Debian e no Ubuntu; greeter e 1 no Arch (o PKGBUILD passa os dois)
    greeter_user=${SWAYDESK_GREETER_USER:-_greetd} greeter_vt=${SWAYDESK_GREETER_VT:-7}
    files=$root/packaging/files
    install -Dm644 "$files/greeter-sway.conf" "$dest/usr/share/swaydesk-greeter/sway.conf"
    # O XDG_CONFIG_HOME da tela no US internacional: o GTK lê dele a tabela com ' + c dando ç (a do ~/.XCompose)
    install -Dm644 "$root/rust/common/src/intl.XCompose" "$dest/usr/share/swaydesk-greeter/intl/gtk-4.0/Compose"
    install -d "$dest/usr/share/swaydesk-greeter" "$dest/usr/lib/systemd/system/greetd.service.d" "$dest/usr/lib/tmpfiles.d"
    # O serviço do PAM das sessões das pessoas: o pacote traz o próprio, o /etc/pam.d/swaydesk-greeter, sem
    # mexer no arquivo do greetd. No Arch (SWAYDESK_GREETER_PAM_STACK=arch, do PKGBUILD), o /etc/pam.d/greetd
    # não chama o gnome-keyring. No Debian e no Ubuntu, é o /etc/pam.d/greetd do Debian 13 (o login, com o
    # LANG do /etc/default/locale, e o gnome-keyring): o greetd do Debian testing trocou o dele por só
    # common-*, e a sessão saía sem o LANG e sem o chaveiro. A sessão do greeter fica com o serviço greetd
    pam_stack=${SWAYDESK_GREETER_PAM_STACK:-debian}
    case $pam_stack in
      arch|debian) ;;
      *) echo "install-tree: SWAYDESK_GREETER_PAM_STACK must be arch or debian" >&2; exit 2 ;;
    esac
    install -Dm644 "$files/greeter-pam-$pam_stack" "$dest/etc/pam.d/swaydesk-greeter"
    edits=(-e "s|@USER@|$greeter_user|g" -e "s|@VT@|$greeter_vt|g"
           -e "s|^@GENERAL@\$|# As sessões das pessoas: o serviço do PAM do pacote (/etc/pam.d/swaydesk-greeter)\\n[general]\\nservice = \"swaydesk-greeter\"\\n|"
           -e "s|^@GREETER_SERVICE@\$|# A sessão da própria tela: o serviço do greetd, sem chaveiro\\nservice = \"greetd\"|")
    sed "${edits[@]}" "$files/greeter-greetd.toml" > "$dest/usr/share/swaydesk-greeter/greetd.toml"
    install -m644 "$files/greeter-greetd.service.conf" "$dest/usr/lib/systemd/system/greetd.service.d/swaydesk-greeter.conf"
    sed "s|@USER@|$greeter_user|g" "$files/greeter-tmpfiles.conf" > "$dest/usr/lib/tmpfiles.d/swaydesk-greeter.conf"
    chmod 644 "$dest/usr/share/swaydesk-greeter/greetd.toml" "$dest/usr/lib/tmpfiles.d/swaydesk-greeter.conf"
    ;;
esac
