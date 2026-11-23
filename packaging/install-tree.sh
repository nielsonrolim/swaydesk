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
SCRIPTS=(session-action sway-brightness sway-caffeine sway-nightlight waybar-notify sway-idle sway-idle-suspend sway-lock
         sway-lock-watch sway-gtklock sway-files sway-shot sway-rec)
# Os módulos em Python: o calendário roda o waybar_holidays.py com o python3, e ele importa os outros dois da
# própria pasta. Sem o bit de execução (o debian/rules os tira do dh_fixperms)
# O bloqueio em Python, no swaydesk-session do Debian e do Ubuntu (o waybar_media, para a mídia na tela)
PYTHON_LOCK=(sway-lock-gtk sway_lock.py sway_pam.py waybar_media.py)
MODULES=(waybar_holidays.py sway_i18n.py desktop_config.py sway-i18n.sh)  # o sway-i18n.sh é incluído com "."
# O swaydesk-desktop: os scripts que o config do Sway e o da Waybar do pacote chamam, além dos do
# swaydesk-scripts, e a sessão (o sway-session e o sway-gpu, que no --system vão para /usr/local/bin). O
# tests/test_packaging.py confere que todo programa que as configs chamam está num pacote
SESSION_SCRIPTS=(sway-after-env sway-if-enabled sway-waybar sway-env-cleanup sway-polkit-agent sway-update-notifier
                 sway-language-packs
                 blueman-agent bing-wallpaper sway-osd sway-super sway-resize sway-master sway-ws waybar-clock
                 waybar-media waybar-volume waybar-battery waybar-brightness waybar-caffeine sway-launcher
                 sway-portal-gtk sway-has-display sway-updates waybar-updates sway-autostart
                 sway-input-method)
SESSION_MODULES=(waybar-icon-lib)  # incluído com "." pelos módulos da Waybar
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
# recomendado e o Adwaita não tem o preferences-desktop-theme
HICOLOR_ICONS=(swaydesk-screenshot.svg swaydesk-screen-record.svg org.swaydesktop.Theme.svg)
SESSION_CONFIGS=(sway/config waybar/config.jsonc waybar/style.css xdg-desktop-portal/sway-portals.conf)  # de home/.config
# Os atalhos: o base.conf e um arquivo por perfil do shortcuts (sway/keys, ao lado do sway/config)
KEY_PROFILES=(base swaydesk i3 hyprland niri dwm)
SYSTEM_SCRIPTS=(sway-session sway-gpu)  # de system/usr/local/bin
# O autostart do XDG (sway-autostart): o target e os drop-ins que pulam, na sessão do Swaydesk, as entradas que
# duplicam o que ele já roda. De home/.config/systemd/user, onde os dotfiles os ligam
AUTOSTART_TARGET=swaydesk-autostart.target
autostart_dropins() { (cd "$root/home/.config/systemd/user" && printf '%s\n' app-*@autostart.service.d/swaydesk.conf); }

if [[ ${1:-} == --sources ]]; then
  for name in "${SCRIPTS[@]}" "${MODULES[@]}" "${SESSION_SCRIPTS[@]}" "${SESSION_MODULES[@]}" "${PYTHON_LOCK[@]}"; do
    echo "home/.local/bin/$name"
  done
  (cd "$root" && printf '%s\n' home/.local/share/locale/*/LC_MESSAGES/sway-desktop.mo)
  for entry in "${CAPTURE_ENTRIES[@]}" "$THEME_ENTRY" "$HELP_ENTRY" "$DISPLAYS_ENTRY"; do echo "home/.local/share/applications/$entry"; done
  for icon in "${HICOLOR_ICONS[@]}"; do echo "home/.local/share/icons/hicolor/scalable/apps/$icon"; done
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

# Nos arquivos de config do pacote, os scripts e os lançadores de ~/.local/bin passam a ser os do libexec
package_paths() {
  sed -e "s|^set \$bin  *~/\.local/bin\$|set \$bin $libexec|" -e "s|~/\.local/bin/|$libexec/|g" "$1"
}
case $pkg in
  swaydesk-common)
    # Os ícones da Symbols Nerd Font (a mesma v3.4.0 que o install.sh baixa), que o Debian não empacota. O
    # Arch tem o ttf-nerd-fonts-symbols no repositório: lá, SWAYDESK_BUNDLED_FONT=0 e o pacote depende dele
    if [[ ${SWAYDESK_BUNDLED_FONT:-1} == 1 ]]; then
      install -Dm644 "$root/packaging/fonts/SymbolsNerdFont-Regular.ttf" \
        "$dest/usr/share/fonts/truetype/swaydesk/SymbolsNerdFont-Regular.ttf"
    fi
    # Os ícones do hicolor, aqui porque a Ajuda (swaydesk-widgets) também os mostra
    for icon in "${HICOLOR_ICONS[@]}"; do
      install -Dm644 "$data/icons/hicolor/scalable/apps/$icon" "$dest/usr/share/icons/hicolor/scalable/apps/$icon"
    done
    # A visão geral, swaydesk(7): o pacote de que todos os outros dependem
    install -Dm644 "$man/swaydesk.7" "$dest/usr/share/man/man7/swaydesk.7"
    # As traduções: os binários procuram em bin/../share/locale, que aqui é /usr/share/locale
    for mo in "$data"/locale/*/LC_MESSAGES/sway-desktop.mo; do
      lang=${mo#"$data/locale/"}
      install -Dm644 "$mo" "$dest/usr/share/locale/$lang"
    done
    ;;
  swaydesk-scripts)
    for name in "${SCRIPTS[@]}"; do
      install -Dm755 "$bin/$name" "$dest$libexec/$name"
    done
    for name in "${MODULES[@]}"; do
      install -Dm644 "$bin/$name" "$dest$libexec/$name"
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
  swaydesk-widgets)
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
  swaydesk-theme)
    # Os modelos e os perfis do dconf ficam em bin/../share/sway-desktop, onde o share_dir os acha
    for file in "$data"/sway-desktop/templates/* "$data"/sway-desktop/dconf/*; do
      install -Dm644 "$file" "$dest/usr/share/sway-desktop/${file#"$data/sway-desktop/"}"
    done
    install -Dm644 "$root/packaging/files/org.freedesktop.impl.portal.desktop.swaydesktop.service" \
      "$dest/usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.swaydesktop.service"
    install -Dm644 "$root/packaging/files/swaydesktop.portal" "$dest/usr/share/xdg-desktop-portal/portals/swaydesktop.portal"
    # O app Tema do Desktop no menu. O nome do arquivo é o app_id do app, que é como a dock acha o ícone. O
    # Exec dos dotfiles é o lançador de ~/.local/bin, que o pacote não tem: aqui, o binário em /usr/bin
    entry=$THEME_ENTRY
    install -d "$dest/usr/share/applications"
    sed 's/^Exec=sway-theme-app$/Exec=swaydesk-theme-app/' "$data/applications/$entry" > "$dest/usr/share/applications/$entry"
    chmod 644 "$dest/usr/share/applications/$entry"
    grep -qx 'Exec=swaydesk-theme-app' "$dest/usr/share/applications/$entry" || {
      echo "install-tree: $entry: no Exec=sway-theme-app line to point at the package binary" >&2
      exit 1
    }
    # O app Telas, do mesmo jeito (o nome do arquivo é o app_id, e o swaydesk-help o abre por ele)
    entry=$DISPLAYS_ENTRY
    sed 's/^Exec=sway-displays$/Exec=swaydesk-displays/' "$data/applications/$entry" > "$dest/usr/share/applications/$entry"
    chmod 644 "$dest/usr/share/applications/$entry"
    grep -qx 'Exec=swaydesk-displays' "$dest/usr/share/applications/$entry" || {
      echo "install-tree: $entry: no Exec=sway-displays line to point at the package binary" >&2
      exit 1
    }
    ;;
  swaydesk-desktop)
    # A sessão "Sway Desktop": só com os pacotes, sem os dotfiles. O sway-session --package abre o Sway com
    # o config daqui, e o setup copia o skel (o home-copy/) para a HOME no primeiro login
    for name in "${SESSION_SCRIPTS[@]}"; do
      install -Dm755 "$bin/$name" "$dest$libexec/$name"
    done
    for name in "${SESSION_MODULES[@]}"; do
      install -Dm644 "$bin/$name" "$dest$libexec/$name"
    done
    for name in "${SYSTEM_SCRIPTS[@]}"; do
      install -Dm755 "$root/system/usr/local/bin/$name" "$dest$libexec/$name"
    done
    # Os nomes que as configs e os scripts usam (sway-dock, waybar-network…) apontam para o binário em Rust,
    # como no install.sh --rust-only: o lançador em Python de cada um diz o componente (sway_rust.exec_rust)
    rust_launchers | while read -r name comp; do
      install -d "$dest$libexec"
      ln -sfn "/usr/bin/swaydesk-$comp" "$dest$libexec/$name"
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
    ;;
  swaydesk-session)
    # O serviço do PAM do bloqueio (o swaydesk-lock e o sway-lock-gtk): o próprio, sem depender do do swaylock
    install -Dm644 "$root/packaging/files/swaydesk-lock-pam" "$dest/etc/pam.d/swaydesk-lock"
    # O bloqueio em Python (GTK 3 + gtk-session-lock), o segundo do sway-lock: no Debian 13, o gtk4-layer-shell
    # 1.0 não tem o bloqueio, e ele é o bloqueio de fato. Ao lado dos módulos do swaydesk-scripts
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
