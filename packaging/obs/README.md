# Repositórios no OBS (openSUSE Build Service)

O projeto `home:nielsonrolim` do OBS compila e publica os pacotes, assinados com a chave do projeto, para:

| Repositório do OBS | Base | Para |
|---|---|---|
| `Debian_13` | `Debian:13/standard` | Debian 13 (trixie) |
| `xUbuntu_26.04` | `Ubuntu:26.04/universe` | Ubuntu 26.04 e Linux Mint 27 |
| `Arch` | `Arch:Extra/standard` | Arch Linux |

Tudo em x86_64. Os pacotes do projeto:
O Debian testing fica de fora até passar numa VM.

- `swaydesk`: um fonte para todos os alvos. O `.dsc` serve ao Debian e ao Ubuntu, e o `PKGBUILD` ao Arch. Os dois usam o mesmo `.orig.tar.xz`, que leva os crates em `rust/vendor`, porque o OBS compila sem rede.
- `swaydesk-archive-keyring` (só Debian e Ubuntu): a chave do projeto e o `.sources` do APT (`keyring/`).

## Arquivos

- `make-source.sh SAÍDA`: os pacotes-fonte do commit atual, em `SAÍDA/swaydesk` e em `SAÍDA/swaydesk-archive-keyring`.
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

Assim o apt e o pacman atualizam de um snapshot para o seguinte, e do último snapshot para o lançamento.

### Lançar uma versão

1. Feche a entrada de cima do `debian/changelog`: a versão `X.Y.Z-1`, a distribuição `unstable` no lugar de `UNRELEASED`, a data e o resumo das mudanças. Faça o commit no `main`.
2. `git tag vX.Y.Z` nesse commit e `git push origin vX.Y.Z`.
3. Gere e envie os pacotes desse commit, como em "Enviar ao OBS" abaixo.
4. **Logo em seguida**, no próximo commit do `main`, suba a versão:
   - a `workspace.package.version` do `rust/Cargo.toml` (e o `Cargo.lock`);
   - uma entrada `X.Y.(Z+1)-1 UNRELEASED` no `debian/changelog`;
   - o `pkgver` do `packaging/arch/PKGBUILD`;
   - o cabeçalho das man pages.

   Sem esse passo, um snapshot de depois da tag sairia `X.Y.Z~git…`, abaixo da versão publicada, e ninguém o receberia. O `version.sh` recusa esse caso, e o `tests/test_packaging.py` confere que as versões batem.

## Enviar ao OBS

**O OBS só recebe snapshots do `main`.** O snapshot de uma branch pode ficar acima do `main` que vem depois dela (um commit da branch feito depois do último merge), e quem o instalasse não receberia as próximas versões até o `main` passar daquela hora. Para testar uma branch, use os pacotes locais nas VMs.

Os builds pesados vão um por vez, sob a trava. O `make-source.sh` precisa do cargo, porque o `cargo vendor` baixa os crates; por isso ele roda num contêiner Debian 13. O contêiner recebe um clone do repositório, com as tags, de onde a versão sai (a tag do lançamento ou a data do commit). Um clone também serve a uma worktree, cujo `.git` aponta para fora dela:

```sh
src=$(mktemp -d) out=$(mktemp -d)
git clone -q . "$src"
flock /run/user/$UID/swaydesk-build.lock docker run --rm -v "$src:/src:ro" -v "$out:/out" debian:13 sh -c "
  apt-get update -qq && apt-get install -y -qq --no-install-recommends git ca-certificates xz-utils dpkg-dev cargo python3 >/dev/null &&
  git config --global --add safe.directory '*' && /src/packaging/obs/make-source.sh /out; chown -R $(id -u):$(id -g) /out"
packaging/obs/push.sh "$out"
```

O clone leva o commit atual (HEAD). O que não estiver commitado fica de fora.

O `push.sh` roda fora do contêiner, com o `osc` configurado (`osc whois` responde e a senha fica no chaveiro). Ele mostra o `osc status` de cada pacote antes do commit. Os resultados aparecem em `osc results home:nielsonrolim swaydesk`.

Na primeira vez, e só quando os alvos mudarem, o meta:

```sh
osc meta prj home:nielsonrolim | packaging/obs/meta.py > /tmp/prj.xml && osc meta prj home:nielsonrolim -F /tmp/prj.xml
osc meta prjconf home:nielsonrolim -F packaging/obs/meta/prjconf
osc meta pkg home:nielsonrolim swaydesk -F packaging/obs/meta/swaydesk.xml
osc meta pkg home:nielsonrolim swaydesk-archive-keyring -F packaging/obs/meta/swaydesk-archive-keyring.xml
```

### Conferir depois do primeiro envio

Antes de passar o endereço a alguém:
- [ ] `osc results home:nielsonrolim` mostra `succeeded` nos 3 alvos do `swaydesk` e nos 2 do keyring (o Arch aparece como `disabled`).
- [ ] As chaves existem nos endereços da seção abaixo (`Debian_13/Release.key` e `Arch/x86_64/home_nielsonrolim_Arch.key`) e têm a impressão digital da seção "A chave".
- [ ] Nas VMs, o `apt update` roda sem aviso de assinatura, e o `pacman -Syu` aceita a assinatura.
- [ ] Um snapshot novo chega como atualização no apt e no pacman.

## Instalar a partir do repositório

### Debian, Ubuntu e Mint

Precisa do `curl` e do `gpg` (num Debian mínimo: `sudo apt install curl gpg`). Na primeira vez, a chave e o `.sources` entram à mão. Use a pasta da distro: `Debian_13` ou `xUbuntu_26.04`, que também serve ao Mint 27. Os comandos vão em cadeia: se a chave baixada não for exatamente uma chave primária, sem subchaves, com a impressão digital da seção "A chave", nada é instalado. Só conferir a impressão digital não basta: no OpenPGP v4, uma subchave tem o mesmo tipo de impressão digital, e uma chave de outra pessoa que trouxesse a nossa como subchave passaria. São os mesmos blocos da página de instalação do site e do README do espelho público.

```sh
repo=https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13
key=$(mktemp) && curl -fsSL "$repo/Release.key" -o "$key" &&
[ "$(gpg --show-keys --with-colons "$key" | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
sudo install -m644 "$key" /usr/share/keyrings/swaydesk-archive-keyring.asc && rm -f "$key" &&
printf 'Types: deb\nURIs: %s/\nSuites: ./\nSigned-By: /usr/share/keyrings/swaydesk-archive-keyring.asc\n' "$repo" |
  sudo tee /etc/apt/sources.list.d/swaydesk.sources >/dev/null &&
sudo apt update && sudo apt install swaydesk-archive-keyring swaydesk-desktop
```

Daí em diante, os dois arquivos ficam por conta do `swaydesk-archive-keyring`, que os atualiza quando a chave mudar.

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
