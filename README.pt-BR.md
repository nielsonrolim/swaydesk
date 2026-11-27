# Swaydesk

[English](README.md)

O Swaydesk é uma sessão de desktop sobre o [Sway](https://swaywm.org), o compositor Wayland de janelas lado a lado, com as peças que fazem dele um desktop completo. Você tem uma barra com popups, uma dock, uma grade de apps, notificações, uma tela de bloqueio e uma tela de login. Cada peça é um programa nativo pequeno, escrito em Rust e GTK 4, e todas usam o mesmo tema.

A documentação completa, com capturas de tela e os atalhos de teclado, está em **https://swaydesk.org** ([Português](https://swaydesk.org/pt-br/) · [English](https://swaydesk.org/en-us/)).

**Versão atual: 0.1.9.** O que muda em cada versão está nas [Novidades](CHANGELOG.pt-BR.md) (desde a 0.1.4; as anteriores, no [CHANGELOG.md](CHANGELOG.md), em inglês) e na [página de lançamentos](https://github.com/nielsonrolim/swaydesk/releases). Depois de instalado, as atualizações chegam com o `apt upgrade` ou o `pacman -Syu` de sempre.

O Swaydesk é um projeto independente, construído sobre o [Sway](https://swaywm.org). Ele não é afiliado ao projeto Sway nem aos seus desenvolvedores, nem endossado por eles.

> **Este repositório é um espelho somente leitura.** O desenvolvimento acontece num repositório privado, e cada mudança mesclada lá é publicada aqui como um commit. Issues e pull requests ainda não estão abertos.

## O que vem nele

| Componente | Pacote | O que faz |
| --- | --- | --- |
| Itens da barra | `swaydesk-calendar`, `swaydesk-weather`, `swaydesk-audio`, `swaydesk-network`, `swaydesk-bluetooth`, `swaydesk-battery`, `swaydesk-brightness`, `swaydesk-caffeine`, `swaydesk-system`, `swaydesk-media`, `swaydesk-language` | Um pacote por módulo da Waybar, com o popup dele |
| Indicador de volume e brilho | `swaydesk-osd` | Ícone, barra e porcentagem na tela por um segundo; o `sway-osd` muda o nível pelas teclas de volume e brilho |
| Troca de janelas | `swaydesk-switcher` | O Alt+Tab: as janelas na ordem de uso, de todos os workspaces |
| Dock | `swaydesk-dock` | Apps fixados e abertos, esconde sozinha, menu do botão direito |
| Grade de apps | `swaydesk-drawer` | A grade de apps em tela cheia, com grupos, busca, arquivos e uma calculadora (o qalc para unidades e conversões) |
| Lançador | `swaydesk-desktop` | O rofi (Alt+Espaço), com apps, janelas abertas, histórico da área de transferência e busca na web |
| Notificações | `swaydesk-notifd` | O serviço de notificações, com os avisos na tela, o histórico e o não perturbe |
| Área de transferência | `swaydesk-clipboard` | O histórico (cliphist), com um popup |
| Captura e gravação | `swaydesk-capture` | Os painéis de captura de tela (área, janela, tela) e de gravação |
| Tema | `swaydesk-theme-app`, `swaydesk-theme-tool`, `swaydesk-portal` | O app Tema: cor de destaque, claro ou escuro e ícones, aplicados ao GTK, ao Qt, ao Sway, à Waybar e ao rofi; as ferramentas de linha de comando que ele usa e o portal que dá a cor de destaque aos apps libadwaita |
| Telas | `swaydesk-displays` | O app Telas: arruma as telas num mapa, com resolução, taxa de atualização, escala e rotação, e um perfil por conjunto de monitores, aplicado ao conectá-los ou ao fechar a tampa |
| Ajuda | `swaydesk-help` | Uma janela com os atalhos do Sway em uso e um cartão por app do desktop |
| Tela de bloqueio | `swaydesk-lock` | Hora, data e a senha em todos os monitores, sobre o papel de parede desfocado |
| Sessão | `swaydesk-session-menu`, `swaydesk-keyring`, `swaydesk-sessionmgr` | O menu da sessão (sair, suspender, reiniciar, desligar), o desbloqueio do chaveiro e o gerenciador de sessão das atualizações |
| Tela de login | `swaydesk-greeter` | Uma tela de login para o greetd, no visual da tela de bloqueio: opcional com outro gerenciador de login, necessária para entrar no modo gráfico sem um |

Cada componente se instala sozinho. Alguns pacotes instalam grupos:
- `swaydesk-bar`: todos os itens da barra, com o indicador de volume e brilho;
- `swaydesk-apps`: os apps Tema, Telas e Ajuda e os painéis de captura;
- `swaydesk-desktop`: a sessão completa, com tudo.

O `swaydesk-widgets`, o `swaydesk-theme` e o `swaydesk-session`, os nomes dos pacotes antes da 0.1.7, instalam os mesmos componentes de antes. O `swaydesk-scripts`, um pacote de transição, saiu na 0.1.10: no Debian e no Ubuntu você pode removê-lo (`sudo apt remove swaydesk-scripts`), e no Arch o `pacman -Syu` o substitui. Se você vem para a 0.1.10 direto da 0.1.6 ou da 0.1.7, atualize com `sudo apt full-upgrade`: o `apt upgrade` e a loja de apps manteriam a versão antiga. O `swaydesk-common` traz os dados, as fontes e os scripts auxiliares em comum. No Debian e no Ubuntu, o `swaydesk-desktop` também traz o `swaydesk-efi`, que mantém funcionando com o Secure Boot o caminho de reserva do firmware (`EFI/BOOT`) dos sistemas instalados por uma imagem do Swaydesk. Em qualquer outro sistema, inclusive um que só adicionou este repositório, ele não faz nada: nunca mexe no ESP, nas entradas de boot nem nos arquivos de outro sistema. Para desligá-lo: `sudo mkdir -p /etc/swaydesk && echo off | sudo tee /etc/swaydesk/efi-fallback`. Para ver por que ele agiu ou não, rode `sudo SWAYDESK_EFI_VERBOSE=1 /usr/libexec/swaydesk/swaydesk-efi-fallback`: ele diz qual verificação o parou, ou o que mudou.

## Como as peças se encaixam

- O `swaydesk-desktop` instala a sessão **Sway Desktop**, que você escolhe no gerenciador de login (ou no `swaydesk-greeter`), e depende de todos os pacotes acima.
- No login, a sessão abre o Sway com a configuração do Swaydesk. Ela aplica o tema e inicia a Waybar, a dock, o serviço de notificações, o indicador na tela, a troca de janelas, o histórico da área de transferência e os serviços da sessão (inatividade, bloqueio, chaveiro). Os popups, a grade de apps e os painéis de captura abrem quando chamados.
- O app Tema grava um só conjunto de cores para todos os componentes: trocar a cor de destaque muda o desktop inteiro.
- As suas configurações ficam em `~/.config/swaydesk/`, e as atualizações dos pacotes nunca as sobrescrevem.

## Instalação

Os pacotes vêm de um repositório no [Open Build Service](https://build.opensuse.org/), assinado com a chave do projeto. O desktop que você já tem continua na tela de login, e as atualizações chegam com o `apt upgrade` ou o `pacman -Syu` de sempre.

Precisa de Debian 13 ou testing, Ubuntu 26.04, Linux Mint 27 ou Arch Linux, de 64 bits (x86_64), e de uma placa de vídeo Intel, AMD ou NVIDIA com driver para Wayland. Escolha a sua distro e rode um passo por vez:

Os mesmos passos, com um botão de copiar em cada comando, estão na [página de instalação](https://swaydesk.org/pt-br/install/).

### Debian 13

1. Instale o `curl`, que baixa o pacote do repositório.

   ```bash
   sudo apt install curl
   ```

2. Baixe o pacote `swaydesk-archive-keyring`: ele adiciona o repositório e a chave do projeto.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13/all/swaydesk-archive-keyring_2026.11.20_all.deb -o ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

3. Confira o arquivo baixado. O comando tem de mostrar `OK`; se não mostrar, pare aqui, apague o arquivo e não continue.

   ```bash
   echo "2527bebb378c926e631fb367f2ccb18120cea26617c3a90b859c633cb2845700  $HOME/swaydesk-archive-keyring_2026.11.20_all.deb" | sha256sum -c
   ```

4. Instale-o. Ele põe a chave em `/usr/share/keyrings/` e o repositório em `/etc/apt/sources.list.d/swaydesk.sources`, e daí em diante mantém os dois atualizados.

   ```bash
   sudo dpkg -i ~/swaydesk-archive-keyring_2026.11.20_all.deb && rm ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

Opcional, antes do passo 5: veja a impressão digital da chave instalada (precisa do `gpg`: `sudo apt install gpg`).

```bash
gpg --show-keys /usr/share/keyrings/swaydesk-archive-keyring.asc
```

Tem de aparecer uma chave só (uma linha pub, nenhuma sub), com esta impressão digital logo abaixo do pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. A chave do projeto no OBS: RSA de 4096 bits, válida até 23/01/2029. Se for diferente, pare aqui e remova o repositório e a chave: `sudo apt purge swaydesk-archive-keyring`.

5. Atualize a lista de pacotes e instale o Swaydesk.

   ```bash
   sudo apt update && sudo apt install swaydesk-desktop
   ```

#### Tudo num comando só

Depois do passo 1 (curl), os outros passos encadeados: se o arquivo baixado não for o esperado, nada é instalado.

```bash
deb=swaydesk-archive-keyring_2026.11.20_all.deb
dir=$(mktemp -d) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13/all/$deb -o "$dir/$deb" &&
echo "2527bebb378c926e631fb367f2ccb18120cea26617c3a90b859c633cb2845700  $dir/$deb" | sha256sum -c && sudo dpkg -i "$dir/$deb" && rm -r "$dir" &&
sudo apt update && sudo apt install swaydesk-desktop
```

### Debian testing

O Debian testing é melhor esforço: numa transição de bibliotecas do testing, os pacotes podem quebrar por alguns dias, até uma revisão nova.

1. Instale o `curl`, que baixa o pacote do repositório.

   ```bash
   sudo apt install curl
   ```

2. Baixe o pacote `swaydesk-archive-keyring`: ele adiciona o repositório e a chave do projeto.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_Testing/all/swaydesk-archive-keyring_2026.11.20_all.deb -o ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

3. Confira o arquivo baixado. O comando tem de mostrar `OK`; se não mostrar, pare aqui, apague o arquivo e não continue.

   ```bash
   echo "0fa0971606384baf62a634376006c5b01e52a095795f88c730dc8e07104cb664  $HOME/swaydesk-archive-keyring_2026.11.20_all.deb" | sha256sum -c
   ```

4. Instale-o. Ele põe a chave em `/usr/share/keyrings/` e o repositório em `/etc/apt/sources.list.d/swaydesk.sources`, e daí em diante mantém os dois atualizados.

   ```bash
   sudo dpkg -i ~/swaydesk-archive-keyring_2026.11.20_all.deb && rm ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

Opcional, antes do passo 5: veja a impressão digital da chave instalada (precisa do `gpg`: `sudo apt install gpg`).

```bash
gpg --show-keys /usr/share/keyrings/swaydesk-archive-keyring.asc
```

Tem de aparecer uma chave só (uma linha pub, nenhuma sub), com esta impressão digital logo abaixo do pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. A chave do projeto no OBS: RSA de 4096 bits, válida até 23/01/2029. Se for diferente, pare aqui e remova o repositório e a chave: `sudo apt purge swaydesk-archive-keyring`.

5. Atualize a lista de pacotes e instale o Swaydesk.

   ```bash
   sudo apt update && sudo apt install swaydesk-desktop
   ```

#### Tudo num comando só

Depois do passo 1 (curl), os outros passos encadeados: se o arquivo baixado não for o esperado, nada é instalado.

```bash
deb=swaydesk-archive-keyring_2026.11.20_all.deb
dir=$(mktemp -d) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_Testing/all/$deb -o "$dir/$deb" &&
echo "0fa0971606384baf62a634376006c5b01e52a095795f88c730dc8e07104cb664  $dir/$deb" | sha256sum -c && sudo dpkg -i "$dir/$deb" && rm -r "$dir" &&
sudo apt update && sudo apt install swaydesk-desktop
```

### Ubuntu 26.04 / Mint 27

1. Instale o `curl`, que baixa o pacote do repositório.

   ```bash
   sudo apt install curl
   ```

2. Baixe o pacote `swaydesk-archive-keyring`: ele adiciona o repositório e a chave do projeto.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/xUbuntu_26.04/all/swaydesk-archive-keyring_2026.11.20_all.deb -o ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

3. Confira o arquivo baixado. O comando tem de mostrar `OK`; se não mostrar, pare aqui, apague o arquivo e não continue.

   ```bash
   echo "a34f455e495c69a2c5dd94ff8ce6dd0618ec569d6b010010c2360432f6029020  $HOME/swaydesk-archive-keyring_2026.11.20_all.deb" | sha256sum -c
   ```

4. Instale-o. Ele põe a chave em `/usr/share/keyrings/` e o repositório em `/etc/apt/sources.list.d/swaydesk.sources`, e daí em diante mantém os dois atualizados.

   ```bash
   sudo dpkg -i ~/swaydesk-archive-keyring_2026.11.20_all.deb && rm ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

Opcional, antes do passo 5: veja a impressão digital da chave instalada (precisa do `gpg`: `sudo apt install gpg`).

```bash
gpg --show-keys /usr/share/keyrings/swaydesk-archive-keyring.asc
```

Tem de aparecer uma chave só (uma linha pub, nenhuma sub), com esta impressão digital logo abaixo do pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. A chave do projeto no OBS: RSA de 4096 bits, válida até 23/01/2029. Se for diferente, pare aqui e remova o repositório e a chave: `sudo apt purge swaydesk-archive-keyring`.

5. Atualize a lista de pacotes e instale o Swaydesk.

   ```bash
   sudo apt update && sudo apt install swaydesk-desktop
   ```

#### Tudo num comando só

Depois do passo 1 (curl), os outros passos encadeados: se o arquivo baixado não for o esperado, nada é instalado.

```bash
deb=swaydesk-archive-keyring_2026.11.20_all.deb
dir=$(mktemp -d) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/xUbuntu_26.04/all/$deb -o "$dir/$deb" &&
echo "a34f455e495c69a2c5dd94ff8ce6dd0618ec569d6b010010c2360432f6029020  $dir/$deb" | sha256sum -c && sudo dpkg -i "$dir/$deb" && rm -r "$dir" &&
sudo apt update && sudo apt install swaydesk-desktop
```

Num Ubuntu minimizado, como as imagens de servidor e de nuvem, o dpkg deixa de fora as traduções e os manuais de todos os pacotes, inclusive os pacotes de idioma. O Swaydesk mantém as traduções dele; para o resto, rode `sudo unminimize` antes de instalar um pacote de idioma.

Se o seu sistema não estiver em inglês, instale também as traduções do GTK para a sua língua. Sem elas, algumas janelas, como o seletor de arquivos, ficam em inglês. Para o português, por exemplo (es para o espanhol, de para o alemão, e assim por diante):

```bash
sudo apt install language-pack-gnome-pt
```

A sessão do Swaydesk avisa uma vez, com o comando da sua língua, quando elas faltam. O Debian não precisa disso: ele traz essas traduções junto com o GTK.

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
- O Swaydesk liga os efeitos sozinho no próximo login, pela seção `[effects]` do `~/.config/swaydesk/config.toml`.
- O blur atrás da barra, da dock e dos popups (`layer_blur`) vem desligado por padrão, porque apareceu lixo visual numa VM; ligue com `layer_blur = true`.
- As animações das janelas vêm desligadas; ligue com `animation_duration_ms = 250` (em milissegundos, até 5000) na mesma seção. Precisam do SwayFX 0.6 ou mais novo: no 0.5, um valor acima de 0 dá erro de config (o swaynag avisa, e o resto carrega).

### Requisitos

- Uma placa de vídeo com driver para Wayland: Intel, AMD ou NVIDIA (o driver proprietário funciona; o Sway roda com `--unsupported-gpu`). Numa máquina virtual, o virtio-gpu funciona.
- A gravação de tela usa o VA-API na Intel e na AMD. Nas outras, grava pelo processador.

### Entrar na sessão

Se você usa o GDM, o SDDM ou o LightDM, não precisa configurar nada: escolha a sessão **Sway Desktop** na tela de login que você já tem.

Sem um gerenciador de login, como numa instalação mínima ou de servidor, o sistema abre num console de texto: instale o `swaydesk-greeter`, abaixo, para entrar no modo gráfico.

### A tela de login do Swaydesk

O `swaydesk-greeter` é uma tela de login para o [greetd](https://sr.ht/~kennylevinsen/greetd/), no visual da tela de bloqueio do Swaydesk. Ela é opcional se você já tem o GDM, o SDDM ou o LightDM, e necessária para entrar no modo gráfico num sistema sem gerenciador de login. Instale com `sudo apt install swaydesk-greeter` (Debian, Ubuntu, Mint) ou `sudo pacman -S swaydesk-greeter` (Arch). No Debian e no Ubuntu, ele só se ativa sozinho quando não há outro gerenciador de login configurado; no Arch, ative-o com `sudo systemctl enable greetd`. Para trocar do GDM, do SDDM ou do LightDM, desative o atual e rode `sudo systemctl enable greetd`; para voltar, `sudo systemctl disable greetd` e ative o antigo de novo (por exemplo, `sudo systemctl enable gdm`).

## Usar um componente só no Sway puro

Você não precisa da sessão inteira: instale só o pacote que quer (`sudo apt install swaydesk-dock`, ou `sudo pacman -Syu swaydesk-dock`), e ele traz só o que precisa. Nada muda no seu Sway até você optar, e a sessão do Swaydesk, se você instalá-la depois, inicia os componentes dela por conta própria.

**No Sway puro, inclua o trecho do componente.** Cada pacote que roda algo no Sway traz um em `/usr/share/swaydesk/sway/config.d/`, e nada o lê até você pôr a linha de `include` dele no `~/.config/sway/config`, depois da linha `set $mod`:

```
include /usr/share/swaydesk/sway/config.d/50-swaydesk-dock.conf
```

| Componente | Pacote | Trecho | O que ele põe |
| --- | --- | --- | --- |
| Dock | `swaydesk-dock` | `50-swaydesk-dock.conf` | A dock |
| Grade de apps | `swaydesk-drawer` | `50-swaydesk-drawer.conf` | A grade, no Super+Shift+D |
| Notificações | `swaydesk-notifd` | `50-swaydesk-notifd.conf` | O serviço de notificações, e a central no Super+N |
| Indicador de volume e brilho | `swaydesk-osd` | `50-swaydesk-osd.conf` | O indicador, e as teclas de volume e brilho que o mostram |
| Troca de janelas | `swaydesk-switcher` | `50-swaydesk-switcher.conf` | O Alt+Tab |
| Telas | `swaydesk-displays` | `50-swaydesk-displays.conf` | O arranjo salvo, aplicado quando as telas mudam e quando a tampa abre ou fecha |
| Histórico da área de transferência | `swaydesk-clipboard` | `50-swaydesk-clipboard.conf` | O histórico, no Super+Shift+V |
| Tela de bloqueio | `swaydesk-lock` | `50-swaydesk-lock.conf` | O Ctrl+Alt+L, e o bloqueio depois de 10 minutos sem uso e antes de suspender, pelo swayidle |
| Abas | `swaydesk-theme-tool` | `50-swaydesk-theme-tool.conf` | As abas dos grupos em abas e empilhados mais altas, com o título no centro e as cores que o app Tema do Desktop gera a partir do destaque |

- **Os componentes que ficam rodando** (todos menos o bloqueio) sobem como serviços de usuário do systemd, reiniciados se caírem, e param quando aquele Sway sai. Cada trecho se explica nos comentários e termina com a linha de `exec` simples para usar no lugar, num sistema sem sessão de usuário do systemd.
- **Se o seu config já inicia o componente** com uma linha de `exec` própria, tire essa linha: a dock, a grade, a troca de janelas, as telas e o indicador saem na hora ao lado de uma cópia já rodando, o serviço de notificações espera o outro (o mako, por exemplo) sair, e dois vigias da área de transferência guardariam cada cópia duas vezes.
- **Numa sessão que inicia o `graphical-session.target`** (o niri, ou o Hyprland com o uwsm, por exemplo), ligue o serviço uma vez no lugar do trecho: `systemctl --user enable --now swaydesk-dock` (os nomes dos serviços são os dos pacotes). Nenhum pacote os liga, e eles nunca sobem dentro da sessão do Swaydesk.
- **Itens da barra para a sua própria Waybar:** cada um traz o módulo dele em `/usr/share/swaydesk/waybar/`, por exemplo o `swaydesk-calendar.jsonc`, com as linhas para colar no config da sua Waybar.
- **A cor de destaque nos apps libadwaita** (`swaydesk-portal`): copie o `/usr/share/swaydesk/xdg-desktop-portal/sway-portals.conf` para `~/.config/xdg-desktop-portal/` (ele mantém as linhas do arquivo do próprio Sway; se você já tem um lá, acrescente só a linha do Settings) e entre de novo. Ele precisa do xdg-desktop-portal-gtk e do -wlr, e do `XDG_CURRENT_DESKTOP=sway` no ambiente do systemd do usuário; os comentários do arquivo dizem como. Até lá, o xdg-desktop-portal não pergunta ao portal.

**A tela de bloqueio.** O `swaydesk-lock` sozinho bloqueia a tela e volta assim que ela fica bloqueada, e assim funciona num atalho e no swayidle. Se o seu config já inicia o swayidle, fique com um só: em vez de incluir o trecho do bloqueio, acrescente o timeout e o before-sleep dele à sua própria linha, por exemplo:

```
bindsym Ctrl+Alt+l exec swaydesk-lock
exec swayidle -w timeout 600 swaydesk-lock before-sleep swaydesk-lock
```

**A cafeína.** Fora da sessão do Swaydesk (um Sway puro, com o seu próprio swayidle), a cafeína segura um inibidor de inatividade do Wayland (`swaydesk-caffeinate`) em vez de trocar o swayidle, e o seu swayidle e os tempos dele ficam como estão. Com todos os monitores desativados ou desconectados (ou, no Sway 1.12, com a energia desligada), os tempos de inatividade ainda podem disparar.

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
