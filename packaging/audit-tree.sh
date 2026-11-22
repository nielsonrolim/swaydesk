#!/usr/bin/env bash
# Confere uma árvore que vai ficar pública (o tarball do OBS extraído, o espelho público): nenhum arquivo
# com dados desta máquina. Os padrões saem da máquina na hora, e o script não guarda nenhum:
#   - o e-mail do git (git config user.email), fora o endereço noreply do GitHub;
#   - o nome da máquina (hostname), fora quando ele é o nome de um pacote do projeto (uma máquina chamada
#     swaydesk-desktop): esse nome está nos READMEs e no debian/control de propósito, e não diz nada da máquina;
#   - a pasta pessoal ($HOME);
#   - as duas primeiras pastas do caminho do repositório (como /media/disco), quando não são a HOME.
# Imprime a contagem de arquivos por pasta de topo e, para cada padrão, os arquivos que o contêm (sem o
# trecho, para o dado não ir parar num log). Sai com 1 se algum padrão aparece.
# uso: packaging/audit-tree.sh PASTA
set -euo pipefail
root=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
tree=$(cd "${1:?uso: audit-tree.sh PASTA}" && pwd)

declare -A patterns=()
email=$(git -C "$root" config user.email 2>/dev/null || true)
[[ -n $email && $email != *@users.noreply.github.com ]] && patterns[e-mail do git]=$email
# O do kernel, que sempre existe no Linux; o comando hostname (inetutils) nem sempre está instalado (Arch).
# SWAYDESK_AUDIT_HOSTNAME: o dos testes, que não podem depender do nome desta máquina (vazio vale o da máquina)
host=${SWAYDESK_AUDIT_HOSTNAME:-$(cat /proc/sys/kernel/hostname 2>/dev/null || hostname 2>/dev/null ||
  cat /etc/hostname 2>/dev/null || true)}
host=$(tr -d '[:space:]' <<< "$host")
# Os nomes do projeto: o fonte e os pacotes do debian/control e do PKGBUILD do Arch (um que falte é nada)
project_names() {
  sed -n 's/^\(Source\|Package\): *//p' "$root/debian/control" 2>/dev/null
  sed -n 's/^pkgbase=//p; s/^pkgname=(\(.*\))$/\1/p' "$root/packaging/arch/PKGBUILD" 2>/dev/null | tr ' ' '\n'
}
names=$(project_names || true)  # numa variável: com o pipefail, o grep -q saindo cedo faria o sed falhar (SIGPIPE)
if [[ -z $host ]]; then
  echo "nome da máquina: não encontrado, não conferido"
else
  if grep -qixF -- "$host" <<< "$names"; then
    echo "nome da máquina: é o de um pacote do projeto, não conferido"
  else
    patterns[nome da máquina]=$host
  fi
fi
[[ -n ${HOME:-} && $HOME != / ]] && patterns[pasta pessoal]=$HOME
# O repositório principal, não a worktree (que pode estar numa pasta temporária)
main=$(dirname "$(git -C "$root" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || echo "$root/.git")")
prefix=$(cut -d/ -f1-3 <<< "$main")  # /media/disco de /media/disco/...
[[ $prefix == /*/* && $HOME != "$prefix"* ]] && patterns[caminho do repositório]=$prefix

# Por pasta de topo; numa árvore com uma pasta só (um tarball extraído), pelas pastas dela
echo "arquivos por pasta de topo:"
(cd "$tree" && find . -type f -o -type l) | cut -d/ -f2-3 | sed -E 's|^([^/]+)$|\1|' | sort | uniq -c
found=0
for label in "${!patterns[@]}"; do
  # O nome da máquina como palavra inteira: um nome curto ("debian") apareceria dentro de outras
  words=(); [[ $label == "nome da máquina" ]] && words=(-w)
  hits=$(grep -rlF "${words[@]}" -- "${patterns[$label]}" "$tree" || true)
  if [[ -z $hits ]]; then
    echo "$label: nada"
  else
    found=1
    echo "$label: $(wc -l <<< "$hits") arquivo(s):"
    sed "s|^$tree/|  |" <<< "$hits"
  fi
done
exit "$found"
