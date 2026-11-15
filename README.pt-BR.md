# Swaydesk

[English](README.md)

O Swaydesk é uma sessão de desktop sobre o [Sway](https://swaywm.org), o compositor Wayland de janelas lado a lado, com as peças que fazem dele um desktop completo. Você tem uma barra com popups, uma dock, uma grade de apps, notificações, uma tela de bloqueio e uma tela de login. Cada peça é um programa nativo pequeno, escrito em Rust e GTK 4, e todas usam o mesmo tema.

A documentação completa, com capturas de tela e os atalhos de teclado, está em **https://swaydesk.nielsonrolim.com** ([Português](https://swaydesk.nielsonrolim.com/pt-br/) · [English](https://swaydesk.nielsonrolim.com/en-us/)).

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
| Telas, Ajuda | — | Em breve |

O `swaydesk-common` traz os dados e as fontes em comum, e o `swaydesk-scripts` os scripts auxiliares.

## Como as peças se encaixam

- O `swaydesk-desktop` instala a sessão **Sway Desktop**, que você escolhe no gerenciador de login (ou no `swaydesk-greeter`), e depende de todos os pacotes acima.
- No login, a sessão abre o Sway com a configuração do Swaydesk. Ela aplica o tema e inicia a Waybar, a dock, o serviço de notificações, o indicador na tela, a troca de janelas, o histórico da área de transferência e os serviços da sessão (inatividade, bloqueio, chaveiro). Os popups, a grade de apps e os painéis de captura abrem quando chamados.
- O app Tema grava um só conjunto de cores para todos os componentes: trocar a cor de destaque muda o desktop inteiro.
- As suas configurações ficam em `~/.config/sway-desktop/`, e as atualizações dos pacotes nunca as sobrescrevem.

## Instalação

Os pacotes vêm do repositório `home:nielsonrolim` no [openSUSE Build Service](https://build.opensuse.org), para x86_64. Mais na [página de instalação](https://swaydesk.nielsonrolim.com/pt-br/install/).

A chave que assina o repositório:
- impressão digital `71A5 E6CB 39B9 844E 42CE  42D3 5815 F0B6 5BE1 CFB9`;
- RSA 4096, vence em 23/01/2029.

Os comandos abaixo conferem por você: se a chave baixada não for uma chave só, com essa impressão digital logo abaixo do `pub` e sem subchaves, nada é instalado.

### Debian 13, Ubuntu 26.04 e Linux Mint 27

Precisa do `curl` e do `gpg` (num sistema mínimo: `sudo apt install curl gpg`). No Ubuntu e no Mint, troque `Debian_13` por `xUbuntu_26.04` na primeira linha.

```sh
repo=https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13
key=$(mktemp) && curl -fsSL "$repo/Release.key" -o "$key" &&
[ "$(gpg --show-keys --with-colons "$key" | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
sudo install -m644 "$key" /usr/share/keyrings/swaydesk-archive-keyring.asc && rm -f "$key" &&
printf 'Types: deb\nURIs: %s/\nSuites: ./\nSigned-By: /usr/share/keyrings/swaydesk-archive-keyring.asc\n' "$repo" |
  sudo tee /etc/apt/sources.list.d/swaydesk.sources >/dev/null &&
sudo apt update && sudo apt install swaydesk-archive-keyring swaydesk-desktop
```

Acrescente `swaydesk-greeter` à última linha se quiser a tela de login do Swaydesk (veja abaixo). Daí em diante, o pacote `swaydesk-archive-keyring` cuida da chave e do repositório e os mantém atualizados.

### Arch Linux

No `/etc/pacman.conf`:

```ini
[home_nielsonrolim_Arch]
Server = https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/$arch
```

Depois confira e importe a chave, e instale. A mesma conferência: se a chave baixada não for uma chave só, com essa impressão digital logo abaixo do `pub` e sem subchaves, nada é importado:

```sh
key=$(mktemp) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/x86_64/home_nielsonrolim_Arch.key -o "$key" &&
[ "$(gpg --show-keys --with-colons "$key" | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
sudo pacman-key --add "$key" && rm -f "$key" &&
sudo pacman-key --lsign-key 71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 &&
sudo pacman -Syu swaydesk-desktop noto-fonts pipewire-jack
```

O `noto-fonts` e o `pipewire-jack` respondem de antemão duas escolhas que o pacman faria (uma fonte e o provedor do JACK): o Swaydesk já usa a Noto, e o som dele é o PipeWire. Se você já tem o `jack2` e o pacman perguntar se pode removê-lo, responda s.

Acrescente `swaydesk-greeter` à última linha para ter a tela de login do Swaydesk.

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

O `swaydesk-greeter` é uma tela de login opcional para o [greetd](https://sr.ht/~kennylevinsen/greetd/), no visual da tela de bloqueio do Swaydesk. No Debian e no Ubuntu, ele só se ativa sozinho quando não há outro gerenciador de login configurado; no Arch, ative-o com `sudo systemctl enable greetd`. Para trocar do GDM, do SDDM ou do LightDM, desative o atual e rode `sudo systemctl enable greetd`; para voltar, `sudo systemctl disable greetd` e ative o antigo de novo (por exemplo, `sudo systemctl enable gdm`).

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
