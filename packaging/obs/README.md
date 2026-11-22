# Repositórios no OBS (openSUSE Build Service)

O projeto `home:nielsonrolim` do OBS compila e publica os pacotes, assinados com a chave do projeto, para:

| Repositório do OBS | Base | Para |
|---|---|---|
| `Debian_13` | `Debian:13/standard` | Debian 13 (trixie) |
| `Debian_Testing` | `Debian:Testing/standard` | Debian testing (hoje o forky), em melhor esforço |
| `xUbuntu_26.04` | `Ubuntu:26.04/universe` | Ubuntu 26.04 e Linux Mint 27 |
| `Arch` | `Arch:Extra/standard` | Arch Linux |

O Debian testing é melhor esforço: com o `rebuild="local"`, uma transição de bibliotecas no testing (um soname
novo) deixa os pacotes não instaláveis até uma revisão nova (`-N`). O `.sources` do keyring manda para ele o
Debian sem `VERSION_ID` (o testing e o sid).

Tudo em x86_64. Os pacotes do projeto:

- `swaydesk`: um fonte para todos os alvos. O `.dsc` serve ao Debian e ao Ubuntu, e o `PKGBUILD` ao Arch. Os dois usam o mesmo `.orig.tar.xz`, que leva os crates em `rust/vendor`, porque o OBS compila sem rede.
- `swaydesk-archive-keyring` (só Debian e Ubuntu): a chave do projeto e o `.sources` do APT (`keyring/`).
- `adw-gtk-theme` (só Debian e Ubuntu; o Arch o tem no extra): o tema [adw-gtk3](https://github.com/lassekongo83/adw-gtk3) (LGPL-2.1), dependência do `swaydesk-desktop`. O orig é o tarball do Release do upstream, byte a byte: o `make-source.sh` o baixa da URL do `adw-gtk-theme/upstream` e confere o sha256. Para uma versão nova do upstream, troque a URL e o sha256, abra uma entrada no `debian/changelog` dele e o hash do conteúdo no `tests/test_obs.py`. Enviar antes do `swaydesk` que passou a pedi-lo, senão o `swaydesk-desktop` fica sem dependência.

## Arquivos

- `make-source.sh SAÍDA`: os pacotes-fonte do commit atual, em `SAÍDA/swaydesk`, `SAÍDA/swaydesk-archive-keyring` e `SAÍDA/adw-gtk-theme`.
- `pkgbuild.py`: o `PKGBUILD` do OBS, que é o `packaging/arch/PKGBUILD` com a versão do snapshot e o tarball local.
- `push.sh SAÍDA [PACOTE...]`: envia as pastas ao OBS (`osc commit`).
- `meta/repositories.xml`: os alvos. O `meta.py` põe esses alvos no meta do projeto sem mexer no resto.
- `meta/<pacote>.xml`: o meta de cada pacote.
- `meta/prjconf`: a configuração do projeto. Tem os `Prefer:` para as escolhas que o OBS não faz sozinho ("have choice for ..."), como o `rust` no lugar do `rustup` no Arch.
- `keyring/`: o fonte do `swaydesk-archive-keyring`.

## Versões

O `packaging/version.sh` dá a versão a partir do commit:
- **na tag `v<X.Y.Z>`** (o lançamento), a versão é a própria `X.Y.Z`, que vem do `rust/Cargo.toml`;
- **fora dela**, é um snapshot abaixo do lançamento, que cresce com a data do commit (a do committer, em UTC): `0.1.0~git<AAAAMMDDhhmmss>.<sha7>` no Debian e `0.1.0git<AAAAMMDDhhmmss>.g<sha7>` no Arch.

A data, e não o número de commits: o squash do GitHub junta os commits de uma branch num só, e o `main` ficaria com menos commits que a branch. O committer de um merge é a hora do merge, sempre depois dos commits da branch.

Assim o apt e o pacman atualizam de um snapshot para o seguinte, e do último snapshot para o lançamento. Uma correção só do empacotamento sai como `X.Y.Z-2`.

**Até a 0.1.0, o OBS recebe snapshots do `main`. Depois dela, só lançamentos:** cada atualização é uma versão nova, com os pacotes e as ISOs. Quem instalou a versão estável não pode receber o desenvolvimento no próximo upgrade, então o `push.sh` recusa um snapshot para um pacote que já tem um lançamento no OBS. Os snapshots seguem existindo para os testes locais nas VMs: depois da 0.1.0, são `0.1.1~git…` e `0.1.1git…`, acima da 0.1.0 e abaixo da 0.1.1 e de uma 0.2.0.

### Lançar uma versão

Antes de cada versão e de cada RC (também as revisões e correções abaixo), **todos os testes** numa worktree limpa
do commit exato que sai: `tests/run-all-tests.sh SAÍDA` roda a suíte em Python com o Sway headless (nunca
`SWAY_DESKTOP_TESTS_SWAY=0`, que pula os testes de GTK), o `cargo test` do workspace inteiro nos contêineres do
Debian 13 e do Arch (com o limite de memória) e cada `rust/scripts/headless-*.sh` (nas duas implementações quando aceita `[rust|python]`), e imprime um
resumo. O `headless-lock` sai como SKIP sem uma imagem com o gtk4-layer-shell 1.2+, e fica para a rodada de VM. Uma falha
segura a versão até ser corrigida ou explicada; o resumo vai junto do pedido de ok ao usuário.

1. O commit de lançamento, num PR que só é mesclado com o ok do usuário (o `mirror.sh` publica o `packaging/public` no espelho):
   - feche a entrada de cima do `debian/changelog`: a versão `X.Y.Z-1`, a distribuição `unstable` no lugar de `UNRELEASED`, a data e o resumo das mudanças;
   - a entrada `## X.Y.Z — AAAA-MM-DD` no topo do `packaging/public/CHANGELOG.md`, que é a nota do Release;
   - a linha "Current release" / "Versão atual" do `packaging/public/README.md` e do `README.pt-BR.md`. O `tests/test_mirror.py` confere que ela é a versão de cima do `CHANGELOG.md`;
   - o rodapé do site, com o builder3, junto com a publicação;
   - se o [adw-gtk3](https://github.com/lassekongo83/adw-gtk3/releases) tem versão nova: atualize o `adw-gtk-theme` (a URL e o sha256 do `upstream`, conferido contra o `digest` do Release no GitHub, uma entrada no `debian/changelog` dele e o hash no `tests/test_obs.py`) e envie-o ao OBS antes do `swaydesk`.
2. `git tag -a vX.Y.Z -m "Swaydesk X.Y.Z"` nesse commit e `git push origin vX.Y.Z`.
3. Gere e envie os pacotes desse commit, como em "Enviar ao OBS" abaixo.
4. Rode o `packaging/obs/check-keyring.sh`: o `.deb` do keyring publicado ainda tem o hash dos READMEs.
5. Publique o espelho com a tag (`packaging/mirror.sh --tag vX.Y.Z`, antes com `--dry-run`) e crie o Release no espelho público com a entrada do `packaging/public/CHANGELOG.md`.
6. **Logo em seguida**, no próximo commit do `main`, suba a versão:
   - a `workspace.package.version` do `rust/Cargo.toml` (e o `Cargo.lock`);
   - uma entrada `X.Y.(Z+1)-1 UNRELEASED` no `debian/changelog`;
   - o `pkgver` do `packaging/arch/PKGBUILD`;
   - o cabeçalho das man pages.

   Sem esse passo, um snapshot de depois da tag sairia `X.Y.Z~git…`, abaixo da versão publicada, e ninguém o receberia. O `version.sh` recusa esse caso, e o `tests/test_packaging.py` confere que as versões batem.

7. As ISOs, quando as versões finais estiverem prontas e testadas (pode ser dias depois, com o `main`
   já adiante: o script não depende do commit): `packaging/release-isos.sh vX.Y.Z PASTA
   debian13=… [debiantesting=…] ubuntu2604=… arch=…` (antes com `--dry-run`; a de Debian Testing é opcional). Ele as
   anexa ao Release do espelho com os nomes `swaydesk-X.Y.Z-debian13-amd64.iso`,
   `swaydesk-X.Y.Z-debiantesting-amd64.iso`, `swaydesk-X.Y.Z-ubuntu26.04-amd64.iso` e `swaydesk-X.Y.Z-arch-x86_64.iso`
   e um `SHA256SUMS` (só SHA-256, sem GPG), baixa de volta e confere, e põe uma seção "ISOs" no fim do texto do
   Release. Uma ISO com mais de 2 GiB (o limite de um arquivo de Release no GitHub) **não é publicada**, nem no
   Release nem na página de download do site: o script avisa qual ficou de fora, com o tamanho, e segue com as
   outras. A `PASTA` recebe gigabytes: fica no volume de build, fora do `/tmp`. O `SHA256SUMS` no mesmo Release protege contra
   um download corrompido ou cortado, não contra quem controla o Release: não é uma assinatura. Se algo falhar
   depois do início do envio, o script lista os `gh release delete-asset` para tirar o que subiu; rode-os e
   repita com a `PASTA` vazia.

   Uma ISO que fica pronta depois das outras entra com `packaging/release-isos.sh --add vX.Y.Z PASTA arch=…`
   (antes com `--dry-run`), numa `PASTA` nova e vazia. O Release precisa já ter o `SHA256SUMS` e a seção "ISOs",
   e não pode ter essa ISO. As linhas do `SHA256SUMS` publicado ficam byte a byte, e a nova vai no fim. O
   script envia a ISO e um `SHA256SUMS.new`, baixa os dois e confere antes de mexer no `SHA256SUMS`: uma ISO
   estragada no envio nunca chega a ser listada. A troca não usa o `--clobber`, que apagaria o `SHA256SUMS`
   antes de subir o novo. São dois renomes pela API: o antigo vira `SHA256SUMS.old`, o `.new` vira
   `SHA256SUMS`, e o `.old` sai. **Entre os dois renomes (uma fração de segundo), quem baixar o `SHA256SUMS`
   recebe um 404**: o GitHub não troca dois arquivos de uma vez, e esse intervalo foi aceito. Depois da troca,
   o script baixa todas as ISOs listadas e confere todas, e só então acrescenta a linha da ISO na seção do
   texto, na ordem das distros. Se o segundo renome falhar, o antigo volta ao nome. Uma sobra (`SHA256SUMS.new` ou `.old`) de uma
   execução interrompida faz o `--add` recusar até ser tirada. A conferência final baixa todas as ISOs do Release: a `PASTA` precisa de
   espaço para todas (uns 8 GB com quatro).

   Uma ISO já publicada que precisa de uma correção é trocada com `packaging/release-isos.sh --replace vX.Y.Z
   PASTA arch=… [ubuntu2604=…]` (antes com `--dry-run`), numa `PASTA` nova e vazia. A ISO tem de estar no
   Release e no `SHA256SUMS`. Cada nova sobe como `NOME.new` junto de um `SHA256SUMS.new` (só as linhas das
   trocadas mudam, no mesmo lugar), e tudo é baixado e comparado byte a byte antes de qualquer troca. Logo
   antes das trocas, o `SHA256SUMS` publicado é baixado de novo: se mudou desde o começo, o script para sem
   trocar nada. As trocas são renomes pela API (`NOME` → `NOME.old`, `NOME.new` → `NOME`, o `SHA256SUMS` por
   último); **entre a troca de uma ISO e a do `SHA256SUMS` (segundos), a ISO nova não confere com o
   `SHA256SUMS` antigo**. Depois, todas as ISOs são baixadas e conferidas, e só então os `.old` saem (até lá,
   dá para voltar atrás). Com várias ISOs, uma falha no meio das trocas deixa as já trocadas fora do `SHA256SUMS`
   até o conserto à mão: trocar uma ISO por execução evita isso. O texto do Release
   não muda. Fora do `--dry-run`, qualquer modo segura um flock (`$XDG_RUNTIME_DIR/swaydesk-release.lock`):
   duas execuções nesta máquina não mexem no Release ao mesmo tempo; quem usa outra máquina avisa antes.

### Uma revisão só do empacotamento (X.Y.Z-N)

Quando os pacotes de um lançamento precisam sair de novo sem mudar o código (um rebuild, um conserto do
empacotamento), a versão é a mesma com a revisão seguinte: `X.Y.Z-2` no Debian e `pkgver=X.Y.Z`, `pkgrel=2` no
Arch. Os nomes de arquivo mudam, e o CDN do OBS não serve um binário velho guardado com o mesmo nome.

1. Uma branch de manutenção `release/X.Y`, a partir da tag `vX.Y.Z` (o `main` já está na versão seguinte).
   **Numa revisão, só vale o que fica fora do orig:** o `debian/` e o `packaging/arch/` (o `PKGBUILD` e os
   `.install`). O resto (o `install-tree.sh`, o `packaging/files`, as man pages, o código) vem do orig da
   `-1`, que não muda (passo 4): um cherry-pick nesses arquivos passaria na branch e sumiria do pacote, sem
   erro. Uma correção ali pede um patch (`debian/patches` com o quilt e um patch no `prepare()` do
   `PKGBUILD`) ou uma versão nova, `X.Y.(Z+1)`. As ferramentas do OBS (`version.sh`, `make-source.sh`,
   `pkgbuild.py`) rodam do commit, fora do pacote, e podem vir do `main` por cherry-pick.
2. Nela, a entrada `swaydesk (X.Y.Z-N) unstable` no topo do `debian/changelog`, num PR para a `release/X.Y`.
3. A tag `git tag -a vX.Y.Z-N -m "Swaydesk X.Y.Z-N"` nesse commit; o `version.sh` dá então `deb` X.Y.Z-N,
   `arch` X.Y.Z e `revision` N, e o `make-source.sh` passa o N ao `pkgbuild.py` como `pkgrel`.
4. O orig não muda: a mesma versão upstream tem o mesmo `.orig.tar.xz`, byte a byte (regra do Debian; e um
   CDN que guardou o orig antigo não serviria outro com o mesmo nome). Baixe o que está no OBS e passe-o ao
   `make-source.sh`, que o exige numa revisão:

   ```sh
   osc checkout --output-dir /tmp/obs-swaydesk home:nielsonrolim swaydesk
   SWAYDESK_ORIG=/tmp/obs-swaydesk/swaydesk_X.Y.Z.orig.tar.xz packaging/obs/make-source.sh SAÍDA
   ```
   (no contêiner, monte o arquivo e passe o `-e SWAYDESK_ORIG=…`).
5. O resto como num lançamento: `push.sh`, os 3 alvos verdes, o tamanho de cada pacote no índice do repositório
   (o `%CSIZE%` do `.db` do Arch, o `Size:` do `Packages`) igual ao que o download entrega, VMs (`apt upgrade` e
   `pacman -Syu` a partir do `-N-1`) e uma nota no Release `vX.Y.Z` do espelho.

### Uma correção com código (X.Y.Z.W)

Quando um lançamento precisa de uma correção no código (o que está no orig) e o `main` já está na versão
seguinte, a versão ganha uma quarta parte: `X.Y.Z.1`. Ela fica acima da `X.Y.Z` e abaixo da `X.Y.(Z+1)` no dpkg
e no pacman (`tests/test_version.py`), e tem um orig próprio.

1. Uma branch `release/X.Y.Z`, a partir da tag `vX.Y.Z`. Cada correção entra primeiro no `main` e depois na
   branch, por cherry-pick, num PR para a `release/X.Y.Z`. Este suporte à quarta parte (o `version.sh`, o
   `pkgbuild.py`, o `mirror.sh --branch`, o `release-isos.sh`) também vai por cherry-pick.
2. O Cargo só aceita semver, então fica na `X.Y.Z`. A versão dos pacotes vem de uma chave que o Cargo ignora,
   no `rust/Cargo.toml`:

   ```toml
   [workspace.metadata.swaydesk]
   release = "X.Y.Z.1"
   ```
   Com ela, `version.sh` dá o snapshot `X.Y.Z.1~git…` (Arch `X.Y.Z.1git…`), e na tag `vX.Y.Z.1` o lançamento. Nenhum
   componente mostra a versão do Cargo (`tests/test_packaging.py`). `pkgver=X.Y.Z.1` no `PKGBUILD`, as man
   pages com "swaydesk X.Y.Z.1" e o `debian/changelog` com a entrada `X.Y.Z.1-1`.
3. O lançamento como os outros, na branch: PR de lançamento para a `release/X.Y.Z`, a tag
   `git tag -a vX.Y.Z.1 -m "Swaydesk X.Y.Z.1"`, `make-source.sh` (orig novo) e `push.sh` de um clone nessa tag.
4. O espelho: `packaging/mirror.sh --branch release/X.Y.Z --tag vX.Y.Z.1` (HEAD = `origin/release/X.Y.Z`).
   Publica na branch `release/X.Y.Z` do espelho, criada na primeira vez a partir da tag `vX.Y.Z` de lá, e a
   `main` do espelho fica como está. As ISOs vão com `release-isos.sh vX.Y.Z.1` (do `main`).
5. No `main`, depois: a entrada `X.Y.Z.1` do CHANGELOG público, a linha de versão atual dos READMEs e a
   entrada do `debian/changelog`, e o `mirror.sh` de sempre.

O OBS não recompila sozinho: os repositórios têm `rebuild="local"` (`meta/repositories.xml`), e só uma
mudança da nossa fonte gera pacotes. Se uma atualização da distro quebrar uma ABI (no Arch, um soname novo),
a saída é uma revisão `-N`. **Nunca dispare um rebuild (ou um wipe) de um lançamento** no OBS, nem à mão: o
binário novo sai com o mesmo nome do publicado. Sempre uma revisão `-N`. Ao mudar o meta do projeto, confira
com `osc results` que nenhum build começou; se começou, a revisão `-N` vai logo em seguida.

## Enviar ao OBS

**Até a 0.1.0, o OBS só recebe snapshots do `main`; depois dela, só lançamentos.** O snapshot de uma branch pode ficar acima do `main` que vem depois dela (um commit da branch feito depois do último merge), e quem o instalasse não receberia as próximas versões até o `main` passar daquela hora. Para testar uma branch, use os pacotes locais nas VMs.

Os builds pesados vão um por vez, sob a trava. O `make-source.sh` precisa do cargo, porque o `cargo vendor` baixa os crates; por isso ele roda num contêiner Debian 13. O contêiner recebe um clone do repositório, com as tags, de onde a versão sai (a tag do lançamento ou a data do commit). Um clone também serve a uma worktree, cujo `.git` aponta para fora dela:

```sh
src=$(mktemp -d) out=$(mktemp -d)
git clone -q . "$src"
flock /run/user/$UID/swaydesk-build.lock docker run --rm --memory 10g --memory-swap 10g -v "$src:/src:ro" -v "$out:/out" debian:13 sh -c "
  apt-get update -qq && apt-get install -y -qq --no-install-recommends git ca-certificates xz-utils dpkg-dev cargo python3 >/dev/null &&
  git config --global --add safe.directory '*' && /src/packaging/obs/make-source.sh /out; chown -R $(id -u):$(id -g) /out"
packaging/obs/push.sh "$out"
```

O clone leva o commit atual (HEAD). O que não estiver commitado fica de fora.

O `push.sh` roda fora do contêiner, com o `osc` configurado (`osc whois` responde e a senha fica no chaveiro). Ele não precisa do dpkg, e roda também num PC Arch: compara as versões com o `debversion.py` (a ordem do `dpkg --compare-versions`) e extrai os fontes com o `tar` (precisa de `python3`, `tar` e `xz`). Ele mostra o `osc status` de cada pacote antes do commit. Os resultados aparecem em `osc results home:nielsonrolim swaydesk`.

Na primeira vez, e só quando os alvos mudarem, o meta:

```sh
osc meta prj home:nielsonrolim | packaging/obs/meta.py > /tmp/prj.xml && osc meta prj home:nielsonrolim -F /tmp/prj.xml
osc meta prjconf home:nielsonrolim -F packaging/obs/meta/prjconf
osc meta pkg home:nielsonrolim swaydesk -F packaging/obs/meta/swaydesk.xml
osc meta pkg home:nielsonrolim swaydesk-archive-keyring -F packaging/obs/meta/swaydesk-archive-keyring.xml
osc meta pkg home:nielsonrolim adw-gtk-theme -F packaging/obs/meta/adw-gtk-theme.xml
```

### Conferir depois do primeiro envio

Antes de passar o endereço a alguém:
- [ ] `osc results home:nielsonrolim` mostra `succeeded` nos 4 alvos do `swaydesk` e nos 3 do keyring (o Arch aparece como `disabled`).
- [ ] As chaves existem nos endereços da seção abaixo (`Debian_13/Release.key` e `Arch/x86_64/home_nielsonrolim_Arch.key`) e têm a impressão digital da seção "A chave".
- [ ] Nas VMs, o `apt update` roda sem aviso de assinatura, e o `pacman -Syu` aceita a assinatura.
- [ ] Uma versão nova chega como atualização no apt e no pacman.

## Instalar a partir do repositório

### Debian, Ubuntu e Mint

A instalação começa pelo `.deb` do `swaydesk-archive-keyring`, baixado direto do repositório da distro
(`Debian_13`, `Debian_Testing` ou `xUbuntu_26.04`, que também serve ao Mint 27) e conferido pelo sha256 antes do `dpkg -i`. O
pacote põe a chave e o `.sources`, e daí em diante os atualiza pelo próprio apt. São os mesmos blocos dos READMEs
públicos (`packaging/public/README.md` e `README.pt-BR.md`) e da página de instalação do site:

```sh
deb=swaydesk-archive-keyring_2026.11.20_all.deb
dir=$(mktemp -d) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13/all/$deb -o "$dir/$deb" &&
echo "2527bebb378c926e631fb367f2ccb18120cea26617c3a90b859c633cb2845700  $dir/$deb" | sha256sum -c && sudo dpkg -i "$dir/$deb" && rm -r "$dir" &&
sudo apt update && sudo apt install swaydesk-desktop
```

O link leva a versão do keyring (a data da chave), não a do Swaydesk: um lançamento não o muda. Os hashes ficam
em `keyring-published.sha256`; o `tests/test_mirror.py` confere que os READMEs usam essa versão e esses hashes.
Esse arquivo fica fora da pasta `keyring/`: ela inteira é o fonte do pacote, e um arquivo novo nela muda o
conteúdo sem mudar a versão (o `push.sh` recusa, e o `test_obs` aponta antes).

#### O hash do keyring quebra com qualquer rebuild

O sha256 é o do `.deb` que o OBS publicou. **Qualquer** rebuild do `swaydesk-archive-keyring` gera um `.deb`
novo, com outro hash, e a conferência do README passa a falhar para todo mundo: um envio do pacote, um rebuild
manual (`osc rebuild`), uma mudança no `prjconf` ou no meta do projeto que o faça recompilar. O
`rebuild="local"` dos alvos evita os rebuilds automáticos, mas não esses. Quem mexer no keyring atualiza, na
mesma mudança:
1. o `keyring-published.sha256` (o nome do arquivo, se a versão mudou, e o hash);
2. os blocos do Debian e do Ubuntu nos dois READMEs públicos (o `test_mirror` aponta o que ficou para trás);
3. a página de instalação do site, com o builder3.

Depois de publicado, o `packaging/obs/check-keyring.sh` baixa os `.deb` do OBS e compara com o
`keyring-published.sha256`; rode-o depois de qualquer envio e antes de passar os READMEs adiante. O `push.sh` lembra
disso quando envia o keyring.

### Arch

No `/etc/pacman.conf`:

```ini
[home_nielsonrolim_Arch]
Server = https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/$arch
```

A chave e a instalação, com a mesma conferência do Debian antes do `pacman-key --add`: o `--lsign-key` procura a impressão digital também nas subchaves, então sozinho ele assinaria a chave primária de quem trouxesse a nossa como subchave:

```sh
key=$(mktemp) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/x86_64/home_nielsonrolim_Arch.key -o "$key" &&
[ "$(gpg --show-keys --with-colons "$key" | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
sudo pacman-key --add "$key" && rm -f "$key" &&
sudo pacman-key --lsign-key 71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 &&
sudo pacman -Syu swaydesk-desktop
```

O `pacman` não pergunta nada: o `swaydesk-common` e o `swaydesk-desktop` já pedem a fonte (`noto-fonts`) e o JACK do PipeWire (`pipewire-jack`) antes do que dependeria deles. Se você já tem o `jack2`, o `pacman` pergunta se pode trocá-lo pelo `pipewire-jack`: responda que sim.

## A chave

É a chave do projeto `home:nielsonrolim`, que o OBS gera e guarda:
- impressão digital `71A5 E6CB 39B9 844E 42CE  42D3 5815 F0B6 5BE1 CFB9`;
- RSA 4096;
- vence em 23/01/2029.

O OBS a renova perto do vencimento (`osc signkey --extend`). Depois de uma renovação, ou de uma troca:
1. Copie a chave nova (`osc signkey home:nielsonrolim`) para `keyring/swaydesk-archive-keyring.asc`.
2. Ponha uma entrada com a data no `keyring/debian/changelog`.
3. Envie o pacote. Quem tem o `swaydesk-archive-keyring` recebe a chave nova pelo próprio apt.
4. No Arch, refaça o `pacman-key --add`.
5. A versão do keyring mudou, então o link e o hash do `.deb` também: siga "O hash do keyring quebra com
   qualquer rebuild" acima (o `keyring-published.sha256`, os READMEs, o site e o `check-keyring.sh`).
