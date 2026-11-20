#!/bin/sh
# O .sources do APT para a distro do os-release dado. Só as distros que o projeto do OBS compila; outra
# qualquer falha o build, em vez de apontar para a pasta errada. O Debian testing não tem VERSION_ID (o sid
# também não, e usa o mesmo repositório). O Mint 27 instala o pacote do Ubuntu 26.04, que já vem com a pasta dele.
# uso: sources.sh OS_RELEASE
set -eu
ID= VERSION_ID=
. "${1:?uso: sources.sh OS_RELEASE}"
case "$ID:$VERSION_ID" in
  debian:13) dir=Debian_13 ;;
  debian:) dir=Debian_Testing ;;
  ubuntu:26.04) dir=xUbuntu_26.04 ;;
  *) echo "sources.sh: no repository for $ID $VERSION_ID" >&2; exit 1 ;;
esac
cat <<END
Types: deb
URIs: https://download.opensuse.org/repositories/home:/nielsonrolim/$dir/
Suites: ./
Signed-By: /usr/share/keyrings/swaydesk-archive-keyring.asc
END
