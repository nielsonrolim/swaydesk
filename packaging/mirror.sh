#!/usr/bin/env bash
# Publica no espelho público (github.com/nielsonrolim/swaydesk) a árvore filtrada do commit atual da main:
# rust/, packaging/, debian/, po/, o LICENSE e os arquivos que o packaging/install-tree.sh lê (--sources), o
# mesmo filtro do tarball do OBS (make-orig.sh). Os documentos do espelho (READMEs, CHANGELOG) são os .md de
# packaging/public, na raiz.
# Os dotfiles, o install.sh, os componentes em Python, a documentação interna e os testes ficam de fora.
#
# Roda quem fez o merge, com a main local igual à origin/main. Cada envio é um commit novo por cima do anterior,
# com o título do squash da main (sem os "#N", que lá apontariam para outras issues) e uma linha fixa: o corpo, com
# o contexto interno do PR, não vai. Um título que cita uma sessão de trabalho, ou um commit sem um
# tests/session-names.regex que sirva, sai como "Update from the private repository". O autor fica no endereço
# noreply. Nunca força o push. Antes, o packaging/audit-tree.sh confere a árvore e a mensagem: se ele
# acha algo desta máquina, nada é enviado.
#
# uso: packaging/mirror.sh [--dry-run]    com --dry-run, monta, confere e faz o commit numa pasta
#                                         temporária, mostra o resumo e não envia (a pasta fica)
#      packaging/mirror.sh [--dry-run] --tag vX.Y.Z
#                                         o lançamento: a tag precisa estar no HEAD (= origin/main) e na
#                                         origin do privado; publica como sempre e põe no commit novo do
#                                         espelho (ou no atual, sem mudança) a tag anotada "Swaydesk X.Y.Z".
#                                         Com a main já adiante da tag (o aumento de versão logo depois dela),
#                                         a tag num ancestral do HEAD: monta a árvore da tag e põe a tag no
#                                         único commit da main do espelho com essa mesma árvore (o publicado
#                                         no merge dela); nenhum commit novo, a main do espelho fica como está
#      packaging/mirror.sh [--dry-run] --branch release/X.Y.Z [--tag vX.Y.Z.W]
#                                         uma correção de uma versão lançada: publica o HEAD (= a
#                                         origin/release/X.Y.Z do privado) na branch release/X.Y.Z do espelho,
#                                         criada na primeira vez a partir da tag vX.Y.Z do espelho. A main do
#                                         espelho fica como está
#      packaging/mirror.sh --check-identity CLONE
#                                         só confere os commits do CLONE que a origin/main ainda não tem
# MIRROR_URL troca o destino (padrão: git@github.com:nielsonrolim/swaydesk.git)
set -euo pipefail
shopt -s extglob
root=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
url=${MIRROR_URL:-git@github.com:nielsonrolim/swaydesk.git}
name="Nielson Rolim" email=nielsonrolim@users.noreply.github.com

# Os commits novos do espelho (os que a origin/main dele ainda não tem): autor e committer só no noreply, e na
# mensagem nenhum e-mail fora os noreply (o do GitHub e o Co-Authored-By do Claude). O git config deste
# repositório usa o e-mail pessoal: os commits do espelho nunca podem depender dele
check_identity() {  # CLONE [BASE]: os commits de BASE..HEAD (padrão: origin/main..HEAD, ou todos)
  local clone=$1 range=HEAD ids bad
  if [[ -n ${2:-} ]]; then range=$2..HEAD
  elif git -C "$clone" rev-parse -q --verify origin/main >/dev/null; then range=origin/main..HEAD; fi
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

dry=0 tag= branch=
while (($#)); do
  case $1 in
    --dry-run) dry=1 ;;
    --tag) tag=${2:?--tag vX.Y.Z}; shift ;;
    --branch) branch=${2:?--branch release/X.Y.Z}; shift ;;
    --check-identity) check_identity "${2:?CLONE}"; exit ;;
    *) echo "uso: packaging/mirror.sh [--dry-run] [--branch release/X.Y.Z] [--tag vX.Y.Z]" >&2; exit 2 ;;
  esac
  shift
done
if [[ -n $tag && ! $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(\.[0-9]+)?$ ]]; then
  echo "mirror: the tag must be vX.Y.Z or vX.Y.Z.W, not $tag" >&2
  exit 2
fi
if [[ -n $branch && ! $branch =~ ^release/[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "mirror: the branch must be release/X.Y.Z, not $branch" >&2
  exit 2
fi
# Uma correção (vX.Y.Z.W) só vai na branch release/X.Y.Z, e essa branch só recebe tags dela
if [[ -n $branch && -n $tag && $tag != v${branch#release/}.+([0-9]) ]]; then
  echo "mirror: $branch takes only a tag v${branch#release/}.W, not $tag" >&2
  exit 2
fi
if [[ -z $branch && $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "mirror: a patch release tag ($tag) needs --branch release/X.Y.Z" >&2
  exit 2
fi
target=${branch:-main}

git -C "$root" fetch -q origin "$target"
head=$(git -C "$root" rev-parse HEAD)
if [[ $head != "$(git -C "$root" rev-parse -q --verify "refs/remotes/origin/$target" || true)" ]]; then
  if ((!dry)); then
    echo "mirror: HEAD is not origin/$target; publish only what was merged (git switch $target && git pull)" >&2
    exit 1
  fi
  echo "mirror: HEAD is not origin/$target (dry run, going on)"
fi
# A release: the tag is on what's published, here and in the private origin
# src: the commit whose tree is built. HEAD, except for a release whose tag main already went past (published=1):
# then the tag's commit, and the tag goes on the mirror commit that already has its tree
src=$head published=0
if [[ -n $tag ]]; then
  local_tag=$(git -C "$root" rev-parse -q --verify "refs/tags/$tag^{commit}" || true)
  # The commit the origin's tag points at: the peeled line (^{}) of an annotated tag, or the tag's own line
  remote_tag=$(git -C "$root" ls-remote origin "refs/tags/$tag" "refs/tags/$tag^{}" |
               awk '$2 ~ /\^\{\}$/ { peeled = $1 } { first = first ? first : $1 } END { print peeled ? peeled : first }')
  if [[ $local_tag != "$head" ]]; then
    # Only on main, and only a tag the HEAD already contains (a merged commit, published by an earlier run)
    if [[ -z $branch && -n $local_tag ]] && git -C "$root" merge-base --is-ancestor "$local_tag" "$head"; then
      src=$local_tag published=1
      echo "mirror: $tag is on $(git -C "$root" rev-parse --short "$src"), behind HEAD: tagging the mirror commit published from it"
    else
      echo "mirror: $tag isn't on HEAD; tag the merged commit first" >&2
      exit 1
    fi
  fi
  if [[ $remote_tag != "$src" ]]; then
    echo "mirror: $tag isn't in the private origin on this commit; push the tag first" >&2
    exit 1
  fi
fi

work=$(mktemp -d)
cleanup() { if ((dry)); then echo "mirror: dry run kept in $work"; else rm -rf "$work"; fi; }
trap cleanup EXIT

# A árvore: o commit inteiro numa pasta, e dela só a lista (o --sources do próprio commit)
git -C "$root" archive --format=tar --prefix=full/ "$src" | tar -x -C "$work"
{ printf '%s\n' rust packaging debian po LICENSE; "$work/full/packaging/install-tree.sh" --sources; } > "$work/list"
mkdir "$work/tree"
tar -C "$work/full" -c --verbatim-files-from -T "$work/list" | tar -x -C "$work/tree"
rm -rf "$work/tree/packaging/public" "$work/tree/rust/vendor" "$work/tree/rust/target"
# Todos os .md de packaging/public (os READMEs, o CHANGELOG.md): um documento novo ali entra sozinho
# (e fica público: packaging/public é só para documentos públicos)
cp "$work/full/packaging/public/"*.md "$work/tree/"
printf '%s\n' '# Compilação do Cargo' 'rust/target/' > "$work/tree/.gitignore"
rm -rf "${work:?}/full"

# A mensagem: o título do squash, sem os números de PR e de issue do privado, e uma linha fixa
subject=$(git -C "$root" log -1 --format=%s "$src" | sed -E 's/ \(#[0-9]+\)$//; s/#[0-9]+//g; s/  +/ /g; s/ +$//')
[[ -n $subject ]] || subject="Update from the private repository"
# Sem os nomes das sessões de trabalho do privado: o título do squash só existe depois do merge, então é aqui que
# ele é conferido. O padrão vem do tests/session-names.regex do próprio commit, que não é espelhado (no mirror.sh
# ele iria para o público). Com um nome, ou sem o padrão (uma branch de lançamento antiga), a mensagem é a
# genérica: nada vaza e nada para (um lançamento com tag não pode ganhar outro título)
# Na dúvida, a genérica: só um "não casa" limpo (saída 1 do grep) deixa o título passar; um padrão inválido
# (saída 2) não deixa
# Só serve um arquivo de uma linha, sem espaços (o mesmo que o test_mirror exige). No título, também a palavra
# "builder" (ou "builders") sozinha, em minúsculas e sem "::" antes: nos arquivos ela esbarra no ::builder() do
# GTK, mas num título um falso positivo só custa a mensagem genérica (o Builder do GTK, com maiúscula, passa)
session_names=$(git -C "$root" show "$src:tests/session-names.regex" 2>/dev/null) || session_names=
[[ $session_names =~ ^[^[:space:]]+$ ]] || session_names=
names_rc=1
[[ -z $session_names ]] || { grep -qiE -- "$session_names" <<<"$subject" && names_rc=0 || names_rc=$?; }
if [[ -z $session_names ]]; then
  echo "mirror: no usable tests/session-names.regex in $src (missing, or not one line without spaces or CRLF):" \
    "the generic commit message is used" >&2
  subject="Update from the private repository"
elif ((names_rc == 0)); then
  echo "mirror: the squash title names a working session: the generic commit message is used" >&2
  subject="Update from the private repository"
elif ((names_rc != 1)); then
  echo "mirror: tests/session-names.regex is not a valid grep -E pattern: the generic commit message is used" >&2
  subject="Update from the private repository"
elif grep -qE -- '(^|[^:[:alnum:]_])builders?\b' <<<"$subject"; then
  echo "mirror: the squash title names a working session (builder): the generic commit message is used" >&2
  subject="Update from the private repository"
fi
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
first=0 base=
if [[ -n $branch ]]; then
  # A branch de correção: a do espelho, se já existe; senão, uma nova na tag do lançamento que ela corrige
  if [[ -n $(git ls-remote --heads "$url" "refs/heads/$branch") ]]; then
    git clone -q --depth 1 --branch "$branch" "$url" "$work/mirror"
  else
    released=v${branch#release/}
    [[ -n $(git ls-remote --tags "$url" "refs/tags/$released") ]] || {
      echo "mirror: the mirror has neither $branch nor $released to start it from" >&2; exit 1; }
    git -c advice.detachedHead=false clone -q --depth 1 --branch "$released" "$url" "$work/mirror"
    git -C "$work/mirror" switch -q -c "$branch"
    echo "mirror: $branch starts at the mirror's $released"
  fi
  base=$(git -C "$work/mirror" rev-parse HEAD)
elif ((published)); then
  # The whole history of main: the commit with the tag's tree can be any one of them
  git clone -q "$url" "$work/mirror"
else
  git clone -q --depth 1 "$url" "$work/mirror"
  git -C "$work/mirror" rev-parse -q --verify HEAD >/dev/null || { first=1; git -C "$work/mirror" symbolic-ref HEAD refs/heads/main; }
fi
find "$work/mirror" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -a "$work/tree/." "$work/mirror/"
git -C "$work/mirror" add -A
# The release tag on the mirror: never one it already has (nothing is ever forced)
if [[ -n $tag ]] && [[ -n $(git -C "$work/mirror" ls-remote origin "refs/tags/$tag") ]]; then
  echo "mirror: the mirror already has $tag; nothing sent" >&2
  exit 1
fi
publish_tag() {  # [COMMIT]: HEAD by default
  GIT_COMMITTER_NAME=$name GIT_COMMITTER_EMAIL=$email \
    git -C "$work/mirror" -c tag.gpgsign=false tag -a "$tag" -m "Swaydesk ${tag#v}" "${1:-HEAD}"
  if [[ $(git -C "$work/mirror" for-each-ref "refs/tags/$tag" --format='%(taggername) %(taggeremail)') != "$name <$email>" ]]; then
    echo "mirror: the tag has another tagger; nothing sent" >&2
    exit 1
  fi
  echo "tag: $tag on $(git -C "$work/mirror" rev-parse --short "${1:-HEAD}"), tagger $email"
  if ((dry)); then
    echo "mirror: dry run, tag not pushed"
    return
  fi
  git -C "$work/mirror" push -q origin "refs/tags/$tag"
  echo "mirror: tagged $tag"
}
# A tag behind HEAD: the one commit of the mirror's main with exactly the tag's tree (the one published from it),
# made by the noreply identity like every mirror commit. None or several: nothing is sent
if ((published)); then
  want=$(git -C "$work/mirror" write-tree)
  matches=$(git -C "$work/mirror" log --format='%H %T' HEAD | awk -v t="$want" '$2 == t { print $1 }')
  count=$(grep -c . <<<"$matches" || true)
  if ((count != 1)); then
    echo "mirror: $((count)) commits of the mirror's main have $tag's tree (it needs exactly one); nothing sent" >&2
    exit 1
  fi
  if [[ $(git -C "$work/mirror" log -1 --format='%an <%ae>%n%cn <%ce>' "$matches" | sort -u) != "$name <$email>" ]]; then
    echo "mirror: the mirror commit with $tag's tree has another author or committer; nothing sent" >&2
    exit 1
  fi
  echo "mirror: $tag's tree is the mirror's $(git -C "$work/mirror" rev-parse --short "$matches")"
  publish_tag "$matches"
  exit 0
fi
if git -C "$work/mirror" diff --cached --quiet; then
  echo "mirror: nothing changed since the last publication"
  [[ -z $tag ]] || publish_tag
  exit 0
fi
if ((first)); then
  printf '%s\n\n%s\n' "Swaydesk: the public sources" \
    "The Rust components, the Debian and Arch packaging and the translations of Swaydesk." > "$work/message/MESSAGE"
fi
GIT_AUTHOR_NAME=$name GIT_AUTHOR_EMAIL=$email GIT_COMMITTER_NAME=$name GIT_COMMITTER_EMAIL=$email \
  git -C "$work/mirror" -c commit.gpgsign=false commit -q -F "$work/message/MESSAGE"
git -C "$work/mirror" --no-pager log -1 --shortstat --format='%an <%ae>%n%s'
check_identity "$work/mirror" "$base"
echo "commit identity: only $email"
if ((dry)); then
  echo "mirror: dry run, not pushed"
  [[ -z $tag ]] || publish_tag
  exit 0
fi
git -C "$work/mirror" push -q origin "HEAD:refs/heads/$target"
echo "mirror: published $(git -C "$work/mirror" rev-parse --short HEAD) to $target from $(git -C "$root" rev-parse --short "$head")"
[[ -z $tag ]] || publish_tag
