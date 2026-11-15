#!/usr/bin/env bash
# Publica no espelho público (github.com/nielsonrolim/swaydesk) a árvore filtrada do commit atual da main:
# rust/, packaging/, debian/, po/, o LICENSE e os arquivos que o packaging/install-tree.sh lê (--sources), o
# mesmo filtro do tarball do OBS (make-orig.sh). Os READMEs do espelho são os de packaging/public, na raiz.
# Os dotfiles, o install.sh, os componentes em Python, a documentação interna e os testes ficam de fora.
#
# Roda quem fez o merge, com a main local igual à origin/main. Cada envio é um commit novo por cima do
# anterior, com o título do squash da main (sem os "#N", que lá apontariam para outras issues) e uma linha
# fixa: o corpo, com o contexto interno do PR, não vai. O autor fica no endereço noreply. Nunca força o push. Antes, o packaging/audit-tree.sh confere a árvore e a mensagem: se ele
# acha algo desta máquina, nada é enviado.
#
# uso: packaging/mirror.sh [--dry-run]    com --dry-run, monta, confere e faz o commit numa pasta
#                                         temporária, mostra o resumo e não envia (a pasta fica)
#      packaging/mirror.sh --check-identity CLONE
#                                         só confere os commits do CLONE que a origin/main ainda não tem
# MIRROR_URL troca o destino (padrão: git@github.com:nielsonrolim/swaydesk.git)
set -euo pipefail
root=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
url=${MIRROR_URL:-git@github.com:nielsonrolim/swaydesk.git}
name="Nielson Rolim" email=nielsonrolim@users.noreply.github.com

# Os commits novos do espelho (os que a origin/main dele ainda não tem): autor e committer só no noreply, e na
# mensagem nenhum e-mail fora os noreply (o do GitHub e o Co-Authored-By do Claude). O git config deste
# repositório usa o e-mail pessoal: os commits do espelho nunca podem depender dele
check_identity() {
  local clone=$1 range=HEAD ids bad
  git -C "$clone" rev-parse -q --verify origin/main >/dev/null && range=origin/main..HEAD
  ids=$(git -C "$clone" log --format='%an <%ae>%n%cn <%ce>' "$range" | sort -u)
  [[ -n $ids ]] || return 0  # nada novo
  if [[ $ids != "$name <$email>" ]]; then
    echo "mirror: a new commit has another author or committer; nothing sent" >&2
    return 1
  fi
  bad=$(git -C "$clone" log --format=%B "$range" | grep -oE '[[:alnum:]._%+-]+@[[:alnum:].-]+' |
        grep -vxE "$email|noreply@anthropic\.com" || true)
  if [[ -n $bad ]]; then
    echo "mirror: a commit message has an e-mail address other than the noreply ones; nothing sent" >&2
    return 1
  fi
}

dry=0
case ${1:-} in
  --dry-run) dry=1 ;;
  --check-identity) check_identity "${2:?CLONE}"; exit ;;
  '') ;;
  *) echo "uso: packaging/mirror.sh [--dry-run]" >&2; exit 2 ;;
esac

git -C "$root" fetch -q origin main
head=$(git -C "$root" rev-parse HEAD)
if [[ $head != "$(git -C "$root" rev-parse origin/main)" ]]; then
  if ((!dry)); then
    echo "mirror: HEAD is not origin/main; publish only what was merged (git switch main && git pull)" >&2
    exit 1
  fi
  echo "mirror: HEAD is not origin/main (dry run, going on)"
fi

work=$(mktemp -d)
cleanup() { if ((dry)); then echo "mirror: dry run kept in $work"; else rm -rf "$work"; fi; }
trap cleanup EXIT

# A árvore: o commit inteiro numa pasta, e dela só a lista (o --sources do próprio commit)
git -C "$root" archive --format=tar --prefix=full/ "$head" | tar -x -C "$work"
{ printf '%s\n' rust packaging debian po LICENSE; "$work/full/packaging/install-tree.sh" --sources; } > "$work/list"
mkdir "$work/tree"
tar -C "$work/full" -c --verbatim-files-from -T "$work/list" | tar -x -C "$work/tree"
rm -rf "$work/tree/packaging/public" "$work/tree/rust/vendor" "$work/tree/rust/target"
cp "$work/full/packaging/public/README.md" "$work/full/packaging/public/README.pt-BR.md" "$work/tree/"
printf '%s\n' '# Compilação do Cargo' 'rust/target/' > "$work/tree/.gitignore"
rm -rf "${work:?}/full"

# A mensagem: o título do squash, sem os números de PR e de issue do privado, e uma linha fixa
subject=$(git -C "$root" log -1 --format=%s "$head" | sed -E 's/ \(#[0-9]+\)$//; s/#[0-9]+//g; s/  +/ /g; s/ +$//')
[[ -n $subject ]] || subject="Update from the private repository"
mkdir "$work/message"
printf '%s\n\n%s\n' "$subject" "Mirrored from the private repository." > "$work/message/MESSAGE"

# A auditoria, da árvore e da mensagem
echo "== audit: tree"
"$root/packaging/audit-tree.sh" "$work/tree" || { echo "mirror: the tree has data from this machine; nothing sent" >&2; exit 1; }
echo "== audit: commit message"
"$root/packaging/audit-tree.sh" "$work/message" >/dev/null || {
  echo "mirror: the commit message has data from this machine; nothing sent" >&2; exit 1; }
echo "commit message: nothing found"

# O espelho: o último commit, a árvore trocada inteira
# Um repositório novo, ainda vazio, também clona (com um aviso); um erro de rede ou de acesso para aqui
git clone -q --depth 1 "$url" "$work/mirror"
first=0
git -C "$work/mirror" rev-parse -q --verify HEAD >/dev/null || { first=1; git -C "$work/mirror" symbolic-ref HEAD refs/heads/main; }
find "$work/mirror" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -a "$work/tree/." "$work/mirror/"
git -C "$work/mirror" add -A
if git -C "$work/mirror" diff --cached --quiet; then
  echo "mirror: nothing changed since the last publication"
  exit 0
fi
if ((first)); then
  printf '%s\n\n%s\n' "Swaydesk: the public sources" \
    "The Rust components, the Debian and Arch packaging and the translations of Swaydesk." > "$work/message/MESSAGE"
fi
GIT_AUTHOR_NAME=$name GIT_AUTHOR_EMAIL=$email GIT_COMMITTER_NAME=$name GIT_COMMITTER_EMAIL=$email \
  git -C "$work/mirror" -c commit.gpgsign=false commit -q -F "$work/message/MESSAGE"
git -C "$work/mirror" --no-pager log -1 --shortstat --format='%an <%ae>%n%s'
check_identity "$work/mirror"
echo "commit identity: only $email"
if ((dry)); then
  echo "mirror: dry run, not pushed"
  exit 0
fi
git -C "$work/mirror" push -q origin HEAD:main
echo "mirror: published $(git -C "$work/mirror" rev-parse --short HEAD) from $(git -C "$root" rev-parse --short "$head")"
