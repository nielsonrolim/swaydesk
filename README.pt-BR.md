# Swaydesk

[English](README.md)

O Swaydesk é uma sessão de desktop sobre o [Sway](https://swaywm.org), o compositor Wayland de janelas lado a lado, com as peças que fazem dele um desktop completo. Você tem uma barra com popups, uma dock, uma grade de apps, notificações, uma tela de bloqueio e uma tela de login. Cada peça é um programa nativo pequeno, escrito em Rust e GTK 4, e todas usam o mesmo tema.

A documentação completa, com capturas de tela e os atalhos de teclado, está em **https://swaydesk.nielsonrolim.com** ([Português](https://swaydesk.nielsonrolim.com/pt-br/) · [English](https://swaydesk.nielsonrolim.com/en-us/)).

**Versão atual: 0.1.1.** O que muda em cada versão está no [CHANGELOG.md](CHANGELOG.md) (em inglês) e na [página de lançamentos](https://github.com/nielsonrolim/swaydesk/releases). Depois de instalado, as atualizações chegam com o `apt upgrade` ou o `pacman -Syu` de sempre.

> **Este repositório é um espelho somente leitura.** O desenvolvimento acontece num repositório privado, e cada mudança mesclada lá é publicada aqui como um commit. Issues e pull requests ainda não estão abertos.

## O que vem nele

| Componente | Pacote | O que faz |
| --- | --- | --- |
| Barra e popups | `swaydesk-widgets` | Os módulos da Waybar e os popups deles: áudio, rede, Bluetooth, bateria, brilho, mídia, clima, calendário, cafeína e sistema; a troca de janelas (Alt+Tab) e o indicador de volume e brilho na tela |
| Dock | `swaydesk-dock` | Apps fixados e abertos, esconde sozinha, menu do botão direito |
| Grade de apps | `swaydesk-drawer` | A grade de apps em tela cheia, com grupos, busca, arquivos e uma calculadora (o qalc para unidades e conversões) |
| Lançador | `swaydesk-desktop` | O rofi (Alt+Espaço), com apps, janelas abertas, histórico da área de transferência e busca na web |
| Notificações | `swaydesk-notifd` | O serviço de notificações, com os avisos na tela, o histórico e o não perturbe |
| Área de transferência | `swaydesk-widgets` | O histórico (cliphist), com um popup |
| Captura e gravação | `swaydesk-capture` | Os painéis de captura de tela (área, janela, tela) e de gravação |
| Tema | `swaydesk-theme` | O app Tema: cor de destaque, claro ou escuro e ícones, aplicados ao GTK, ao Qt, ao Sway, à Waybar e ao rofi |
| Sessão | `swaydesk-session` | A tela de bloqueio, o menu da sessão (sair, suspender, reiniciar, desligar), o desbloqueio do chaveiro e os serviços da sessão |
| Tela de login | `swaydesk-greeter` | Uma tela de login opcional para o greetd, no visual da tela de bloqueio |
| Ajuda | `swaydesk-widgets` | Uma janela com os atalhos do Sway em uso e um cartão por app do desktop |
| Telas | `swaydesk-theme` | O app Telas: arruma as telas num mapa, com resolução, taxa de atualização, escala e rotação, e um perfil por conjunto de monitores, aplicado ao conectá-los ou ao fechar a tampa |

O `swaydesk-common` traz os dados e as fontes em comum, e o `swaydesk-scripts` os scripts auxiliares.

## Como as peças se encaixam

- O `swaydesk-desktop` instala a sessão **Sway Desktop**, que você escolhe no gerenciador de login (ou no `swaydesk-greeter`), e depende de todos os pacotes acima.
- No login, a sessão abre o Sway com a configuração do Swaydesk. Ela aplica o tema e inicia a Waybar, a dock, o serviço de notificações, o indicador na tela, a troca de janelas, o histórico da área de transferência e os serviços da sessão (inatividade, bloqueio, chaveiro). Os popups, a grade de apps e os painéis de captura abrem quando chamados.
- O app Tema grava um só conjunto de cores para todos os componentes: trocar a cor de destaque muda o desktop inteiro.
- As suas configurações ficam em `~/.config/sway-desktop/`, e as atualizações dos pacotes nunca as sobrescrevem.

## Instalação

Os pacotes vêm de um repositório no [Open Build Service](https://build.opensuse.org/), assinado com a chave do projeto. O desktop que você já tem continua na tela de login, e as atualizações chegam com o `apt upgrade` ou o `pacman -Syu` de sempre.

Precisa de Debian 13, Ubuntu 26.04, Linux Mint 27 ou Arch Linux, de 64 bits (x86_64), e de uma placa de vídeo Intel, AMD ou NVIDIA com driver para Wayland. Escolha a sua distro e rode um passo por vez:

Os mesmos passos, com um botão de copiar em cada comando, estão na [página de instalação](https://swaydesk.nielsonrolim.com/pt-br/install/).

### Debian 13

1. Instale o `curl`, que baixa o pacote do repositório.

   ```bash
   sudo apt install curl
   ```

2. Baixe o pacote `swaydesk-archive-keyring`: ele adiciona o repositório e a chave do projeto.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13/all/swaydesk-archive-keyring_2026.11.15_all.deb -o ~/swaydesk-archive-keyring_2026.11.15_all.deb
   ```

3. Confira o arquivo baixado. O comando tem de mostrar `OK`; se não mostrar, pare aqui, apague o arquivo e não continue.

   ```bash
   echo "5b8a2cb8af4a051bf5d3decdcf66fb0f4fcbb66d2728656969e1d673e4c00821  $HOME/swaydesk-archive-keyring_2026.11.15_all.deb" | sha256sum -c
   ```

4. Instale-o. Ele põe a chave em `/usr/share/keyrings/` e o repositório em `/etc/apt/sources.list.d/swaydesk.sources`, e daí em diante mantém os dois atualizados.

   ```bash
   sudo dpkg -i ~/swaydesk-archive-keyring_2026.11.15_all.deb && rm ~/swaydesk-archive-keyring_2026.11.15_all.deb
   ```

5. Atualize a lista de pacotes e instale o Swaydesk.

   ```bash
   sudo apt update && sudo apt install swaydesk-desktop
   ```

Opcional: veja a impressão digital da chave (precisa do `gpg`: `sudo apt install gpg`).

```bash
gpg --show-keys /usr/share/keyrings/swaydesk-archive-keyring.asc
```

Tem de aparecer uma chave só (uma linha pub, nenhuma sub), com esta impressão digital logo abaixo do pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. A chave do projeto no OBS: RSA de 4096 bits, válida até 23/01/2029.

#### Tudo num comando só

Depois do passo 1 (curl), os outros passos encadeados: se o arquivo baixado não for o esperado, nada é instalado.

```bash
deb=swaydesk-archive-keyring_2026.11.15_all.deb
dir=$(mktemp -d) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13/all/$deb -o "$dir/$deb" &&
echo "5b8a2cb8af4a051bf5d3decdcf66fb0f4fcbb66d2728656969e1d673e4c00821  $dir/$deb" | sha256sum -c && sudo dpkg -i "$dir/$deb" && rm -r "$dir" &&
sudo apt update && sudo apt install swaydesk-desktop
```

### Ubuntu 26.04 / Mint 27

1. Instale o `curl`, que baixa o pacote do repositório.

   ```bash
   sudo apt install curl
   ```

2. Baixe o pacote `swaydesk-archive-keyring`: ele adiciona o repositório e a chave do projeto.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/xUbuntu_26.04/all/swaydesk-archive-keyring_2026.11.15_all.deb -o ~/swaydesk-archive-keyring_2026.11.15_all.deb
   ```

3. Confira o arquivo baixado. O comando tem de mostrar `OK`; se não mostrar, pare aqui, apague o arquivo e não continue.

   ```bash
   echo "b22f785fbd083685c60bdc90ffb2e1d0e91c93db8513c3fc850e62ab36044767  $HOME/swaydesk-archive-keyring_2026.11.15_all.deb" | sha256sum -c
   ```

4. Instale-o. Ele põe a chave em `/usr/share/keyrings/` e o repositório em `/etc/apt/sources.list.d/swaydesk.sources`, e daí em diante mantém os dois atualizados.

   ```bash
   sudo dpkg -i ~/swaydesk-archive-keyring_2026.11.15_all.deb && rm ~/swaydesk-archive-keyring_2026.11.15_all.deb
   ```

5. Atualize a lista de pacotes e instale o Swaydesk.

   ```bash
   sudo apt update && sudo apt install swaydesk-desktop
   ```

Opcional: veja a impressão digital da chave (precisa do `gpg`: `sudo apt install gpg`).

```bash
gpg --show-keys /usr/share/keyrings/swaydesk-archive-keyring.asc
```

Tem de aparecer uma chave só (uma linha pub, nenhuma sub), com esta impressão digital logo abaixo do pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. A chave do projeto no OBS: RSA de 4096 bits, válida até 23/01/2029.

#### Tudo num comando só

Depois do passo 1 (curl), os outros passos encadeados: se o arquivo baixado não for o esperado, nada é instalado.

```bash
deb=swaydesk-archive-keyring_2026.11.15_all.deb
dir=$(mktemp -d) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/xUbuntu_26.04/all/$deb -o "$dir/$deb" &&
echo "b22f785fbd083685c60bdc90ffb2e1d0e91c93db8513c3fc850e62ab36044767  $dir/$deb" | sha256sum -c && sudo dpkg -i "$dir/$deb" && rm -r "$dir" &&
sudo apt update && sudo apt install swaydesk-desktop
```

### Arch Linux

1. Adicione o repositório ao fim do `/etc/pacman.conf` (uma vez só).

   ```bash
   sudo tee -a /etc/pacman.conf <<'EOF'

   [home_nielsonrolim_Arch]
   Server = https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/$arch
   EOF
   ```

2. Baixe a chave do repositório.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/x86_64/home_nielsonrolim_Arch.key -o ~/swaydesk.key
   ```

3. Mostre a impressão digital da chave baixada.

   ```bash
   gpg --show-keys ~/swaydesk.key
   ```

   Tem de aparecer uma chave só (uma linha pub, nenhuma sub), com esta impressão digital logo abaixo do pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. Se for diferente, pare aqui e não continue. A chave do projeto no OBS: RSA de 4096 bits, válida até 23/01/2029.

4. Adicione a chave ao chaveiro do pacman e assine-a localmente, para o pacman confiar nela. Antes, o comando confere a chave de novo e recusa qualquer outra, como no passo 3.

   ```bash
   [ "$(gpg --show-keys --with-colons ~/swaydesk.key | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
   sudo pacman-key --add ~/swaydesk.key &&
   sudo pacman-key --lsign-key 71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 &&
   rm ~/swaydesk.key
   ```

5. Instale o Swaydesk. Se você já tem o `jack2`, o pacman pergunta se pode trocá-lo pelo `pipewire-jack`, o JACK do PipeWire, que o Swaydesk usa: responda s.

   ```bash
   sudo pacman -Syu swaydesk-desktop
   ```

#### Tudo num comando só

Os mesmos passos, encadeados: se a chave baixada não tiver a impressão digital certa, nada é adicionado nem instalado.

```bash
key=$(mktemp) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/x86_64/home_nielsonrolim_Arch.key -o "$key" &&
[ "$(gpg --show-keys --with-colons "$key" | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
sudo pacman-key --add "$key" && rm -f "$key" &&
sudo pacman-key --lsign-key 71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 &&
printf '\n[home_nielsonrolim_Arch]\nServer = https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/$arch\n' | sudo tee -a /etc/pacman.conf >/dev/null &&
sudo pacman -Syu swaydesk-desktop
```


#### Opcional: SwayFX

O [SwayFX](https://github.com/WillPower3309/swayfx) é o Sway com cantos arredondados, blur e sombras, e no Arch pode substituir o Sway com o mesmo config. Ele vem do AUR, por um AUR helper: `yay -S swayfx` (ou `paru -S swayfx`); o pacman troca o `sway` por ele, e o `swaydesk-desktop` continua instalado, porque o `swayfx` provê o `sway`. Para voltar, `sudo pacman -S sway`.

- É mantido por terceiros, fora do suporte do projeto, e uma atualização que traga um wlroots novo pode quebrá-lo até o AUR acompanhar. Se a sessão não abrir depois de uma atualização, volte ao Sway num console de texto (Ctrl+Alt+F2).
- O Swaydesk liga os efeitos sozinho no próximo login, pela seção `[effects]` do `~/.config/sway-desktop/config.toml`.
- O blur atrás da barra, da dock e dos popups (`layer_blur`) vem desligado por padrão, porque apareceu lixo visual numa VM; ligue com `layer_blur = true`.

### Requisitos

- Uma placa de vídeo com driver para Wayland: Intel, AMD ou NVIDIA (o driver proprietário funciona; o Sway roda com `--unsupported-gpu`). Numa máquina virtual, o virtio-gpu funciona.
- A gravação de tela usa o VA-API na Intel e na AMD. Nas outras, grava pelo processador.

### Entrar na sessão

Se você usa o GDM, o SDDM ou o LightDM, não precisa configurar nada: escolha a sessão **Sway Desktop** na tela de login que você já tem.

### Opcional: a tela de login do Swaydesk

O `swaydesk-greeter` é uma tela de login opcional para o [greetd](https://sr.ht/~kennylevinsen/greetd/), no visual da tela de bloqueio do Swaydesk. Instale com `sudo apt install swaydesk-greeter` (Debian, Ubuntu, Mint) ou `sudo pacman -S swaydesk-greeter` (Arch). No Debian e no Ubuntu, ele só se ativa sozinho quando não há outro gerenciador de login configurado; no Arch, ative-o com `sudo systemctl enable greetd`. Para trocar do GDM, do SDDM ou do LightDM, desative o atual e rode `sudo systemctl enable greetd`; para voltar, `sudo systemctl disable greetd` e ative o antigo de novo (por exemplo, `sudo systemctl enable gdm`).

## Compilar a partir do código

Os componentes são um workspace do Cargo em `rust/` (Rust 1.85 ou mais novo, GTK 4.14 ou mais novo):

```sh
# Debian / Ubuntu
sudo apt install cargo rustc pkgconf libgtk-4-dev libgtk4-layer-shell-dev
# Arch
sudo pacman -S --needed cargo gtk4 gtk4-layer-shell pkgconf

cd rust
cargo build --release
cargo test
```

Os binários ficam em `rust/target/release/swaydesk-*`. Os pacotes os juntam aos arquivos da sessão:
- o `packaging/install-tree.sh` instala os arquivos de cada pacote, e as duas distros o usam;
- o `packaging/make-orig.sh` gera o tarball de origem (com os crates copiados para dentro, para compilar sem rede);
- o `debian/` é o empacotamento do Debian e do Ubuntu, e o `packaging/arch/PKGBUILD`, o do Arch.

São os mesmos arquivos que o repositório do OBS usa. As traduções ficam em `po/`.

## Licença

[MIT](LICENSE)
