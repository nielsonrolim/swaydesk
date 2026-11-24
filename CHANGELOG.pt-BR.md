# Novidades

O que muda em cada versão do Swaydesk, em português a partir da 0.1.4. As versões anteriores estão no [CHANGELOG.md](CHANGELOG.md), em inglês. Cada versão sai primeiro em pacotes e, logo depois, em imagens de instalação. As atualizações chegam pelo `apt upgrade` ou pelo `pacman -Syu`.

## 0.1.6 — 2026-11-24

Uma animação de arraste na dock, um app Tema do Desktop que mantém as suas escolhas e um tema aplicado de fora da sessão que chega à sessão aberta. As imagens de instalação vêm com a 0.1.6.1.

### Novo

- **A dock anima o arraste**: o ícone arrastado levanta e segue o ponteiro, os outros abrem espaço deslizando, e ao soltar ele assenta no lugar. Desliga com `gtk-enable-animations=false`.

### Mudanças

- **Debian 13: um `apt install swaydesk-desktop` simples não puxa mais um segundo agente do polkit (ukui-polkit)**, que trazia umas 50 bibliotecas (OpenCV e GDAL entre elas) e nunca rodava no Sway. O agente do polkit agora vem primeiro nas dependências do pacote, e o apt escolhe o mate-polkit.

### Correções

- **O Tema do Desktop mantém o que você escolheu**:
  - Uma cor de destaque própria, definida com `swaydesk-theme set … --accent #hex` ou a do próprio tema, aparece como uma amostra "Personalizada", selecionada, e aplicar outro estilo a mantém. Antes, ela abria como a amostra mais próxima, com Aplicar já aceso.
  - Abrir o app e fechar sem mexer em nada não muda mais a variante aplicada.
  - O aviso sobre um `gtk.css` que não é do Swaydesk, ou sobre outro desktop instalado, agora aparece ao abrir o app, e não só depois de Aplicar.
  - Com a integração de desktop do snapd instalada, escolher o adw-gtk3 mostra um aviso: os apps snap não o trazem, e o snapd vai pedir a cada login para instalar um snap de tema.
- **O `swaydesk-theme set` rodado fora da sessão** (por ssh, pelo cron ou num TTY) agora grava as configurações que a sessão aberta lê. Os apps GTK 4 e o portal do desktop acompanham na hora. A barra e o próprio Sway pegam a mudança no próximo login, como o comando avisa.
- **A tela de bloqueio** não registra mais linhas `Gdk-CRITICAL` a cada bloqueio.

### Imagens de instalação

A 0.1.6 não traz imagens novas. Estas mudanças chegam a elas na 0.1.6.1:

- **Debian 13 e Arch:** os arquivos do instalador não ficam mais em `/etc/calamares` no sistema instalado.
- **Arch:**
  - Com um fuso no Brasil, os números e as datas do sistema instalado ficam em português, como no Debian.
  - A página de teclado do instalador começa no layout US.
  - O menu do GRUB instalado não lista mais todas as entradas de boot do firmware: as entradas de BootNext do GRUB 2.16 ficam desligadas.

### Atualizando da 0.1.5

- **No Debian 13, instalado pelos pacotes ou por uma imagem do Swaydesk anterior à 0.1.5**, pode haver um segundo agente do polkit (ukui-polkit) e umas 50 bibliotecas que vieram com ele. `dpkg -l ukui-polkit` mostra se ele está instalado. Eles não são usados no Swaydesk. Para remover o agente e o arquivo de autoinício dele, rode `sudo apt purge ukui-polkit`. Depois, o `sudo apt autoremove` remove os pacotes de que nada mais precisa, no sistema todo, e não só esses: leia a lista antes de confirmar.
- O `swaydesk-theme set` de fora da sessão chega a uma sessão aberta depois da atualização; entre de novo uma vez.

## 0.1.5 — 2026-11-23

ç e acentos no Chromium e nos apps Electron pelo fcitx5, ligado por padrão nas imagens de instalação, e uma tela de bloqueio que não usa mais o swaylock, e por isso uma senha com acento a destrava.

### Novo

- **ç e acentos no Chromium e nos apps Electron (fcitx5)**: no Wayland, o Chromium, o Chrome, o Vivaldi, o Brave e os apps Electron (VS Code, Slack, Discord) ignoram o `~/.XCompose` e compõem com uma tabela própria, e `'` + `c` dava `ć`. Com `input_method = true` no `[enabled]` do `~/.config/swaydesk/config.toml` e o pacote `fcitx5` instalado, a sessão sobe o fcitx5. Os navegadores abertos com `--enable-wayland-ime --wayland-text-input-version=3` passam a dar `ç`.
  - A opção vem **desligada nos pacotes e ligada nas imagens de instalação do Debian 13 e do Arch**. As imagens trazem o `/etc/swaydesk/config.toml`, uma nova camada do sistema para os padrões do `[enabled]`. Uma chave no seu próprio `config.toml` sempre vence.
  - O fcitx5 não atrapalha. Os atalhos dele ficam desligados (o Super+Espaço continua trocando o layout no Sway), assim como o ícone na bandeja e os avisos. Ele usa o layout de teclado da sessão, e os apps GTK continuam compondo sozinhos.
  - No Arch, os arquivos de opções que os lançadores dos navegadores leem (`chromium-flags.conf`, `code-flags.conf`…) são criados em `~/.config`, só para o que está instalado e só se não existirem. Um navegador instalado no meio da sessão ganha o arquivo no próximo login.
  - Nada acontece se outro método de entrada (o IBus, por exemplo) estiver configurado ou rodando. Para desligar, `input_method = false`, a partir do próximo login.

### Mudou

- **A tela de bloqueio não usa mais o swaylock**, que não tem teclas mortas: uma senha com acento digitado como `´` e depois a letra nunca o destravava.
  - O bloqueio tenta primeiro o próprio Swaydesk; no Debian 13, é a versão em GTK 3, que agora faz parte do `swaydesk-session`. O último recurso agora é o **gtklock**, dependência em todas as distribuições.
  - Se um bloqueio parar sem desbloquear, outro assume, e a área de trabalho nunca aparece no meio.
  - Se nenhum bloqueio conseguir subir, uma notificação crítica avisa que a tela não está bloqueada.
- **A tela de bloqueio tem regras de login próprias**, o `/etc/pam.d/swaydesk-lock` (iguais às do login do sistema), e não depende mais do arquivo do swaylock.
- **O campo de senha nunca passa por um método de entrada**, mesmo com o fcitx5 ou o IBus rodando, e as teclas vão direto para o campo.
- **Senha errada**: a tela de bloqueio agora mostra tudo o que o sistema diz. No Arch, depois de três senhas erradas, isso quer dizer "The account is locked due to 3 failed logins. (10 minutes left to unlock)", e a mensagem fica na tela. A espera curta depois de três erros agora tem contagem ("Muitas tentativas: tente de novo em N s").

### Corrigido

- **Arrastar um ícone fixado na dock** agora acompanha o ponteiro: o ícone corria na frente dele, e um arraste lento ou um ícone solto além de uma das pontas da dock parava antes de onde você o soltou.

### Imagens de instalação

- As imagens do Debian 13 e do Arch trazem o `fcitx5` e ligam o método de entrada pelo `/etc/swaydesk/config.toml`, na sessão live e no sistema instalado (veja **Novo**).

### Atualizando da 0.1.4

- Depois da atualização, nenhum pacote do Swaydesk precisa mais do swaylock. Ele continua instalado: o sway só o sugere, e por isso o `apt autoremove` e o `pacman -Qdt` não mexem nele. Se nada mais seu o usar, remova com `sudo apt remove swaylock` no Debian e no Ubuntu, ou `sudo pacman -Rns swaylock` no Arch.

## 0.1.4 — 2026-11-23

A cor de destaque chega aos apps GTK, os apps da pasta de início automático abrem com a sessão, os computadores sem bateria ganham os perfis de energia, e os sistemas instalados pelas imagens 0.1.3.x voltam a dar boot com o Secure Boot depois de perder a entrada de boot.

### Novo

- **Cor de destaque nos apps GTK**: a cor de destaque, escolhida por você ou tirada do papel de parede, agora também colore os apps GTK 3 e GTK 4, pelos arquivos `~/.config/gtk-3.0/gtk.css` e `~/.config/gtk-4.0/gtk.css`. Todo desktop lê esses arquivos, então o Swaydesk só os grava quando nenhuma outra sessão de desktop (GNOME, KDE Plasma…) está instalada. Um `gtk.css` que você editou não é tocado. Nos dois casos, o `swaydesk-theme set` mostra um aviso, e o Tema do Desktop também, quando você aplica um tema, na mensagem do topo da janela.
- **adw-gtk3**: o `swaydesk-desktop` agora traz o pacote `adw-gtk-theme` (o tema [adw-gtk3](https://github.com/lassekongo83/adw-gtk3), que segue a cor de destaque), e as contas novas usam o `adw-gtk3-dark` quando o tema Mint-Y não está instalado. A família adw-gtk3 aparece no Tema do Desktop. No Arch, ele vem do repositório `extra`. No Ubuntu com o snapd-desktop-integration (o Ubuntu Desktop), as contas novas usam o `Adwaita-dark`, como na 0.1.3.1, e não o `adw-gtk3-dark`: os snaps não têm o adw-gtk3, e o snapd pedia a cada login para instalar um snap de terceiros do tema ("Some required themes are missing") e abria um pedido de senha ao abrir um app snap. O adw-gtk3 continua instalado e pode ser escolhido no Tema do Desktop; escolhê-lo traz o pedido do snapd de volta. Para voltar ao Adwaita depois de experimentar o adw-gtk3, escolha o Adwaita (Escuro) no Tema do Desktop.
- **Perfis de energia em desktops**: num computador sem bateria, o popup Sistema ganha a linha **Perfil de energia**, com os perfis que o power-profiles-daemon oferece. O popup da bateria, que tem os mesmos botões, não aparece sem bateria. Ponha `power_profiles_in_system = "always"` no `config.toml` para ter a linha também nos notebooks, ou `"never"` para escondê-la; o padrão, `"auto"`, só a mostra sem bateria.
- **Animações de janela no SwayFX 0.6**: `animation_duration_ms` em `[effects]` (de 0 a 5000, padrão 0, desligado) define a duração das animações de abrir e fechar do SwayFX. Precisa do SwayFX 0.6: no 0.5, qualquer valor acima de 0 é um erro de configuração que a barra de erro do Sway mostra, e o resto da configuração carrega normalmente.
- **Pacotes de idioma no Ubuntu**: no Ubuntu e no Linux Mint instalados pelos pacotes, um aviso único mostra o comando que instala as traduções do GTK para o seu idioma (`sudo apt install language-pack-gnome-<idioma>`), para o seletor de arquivos e os diálogos do GTK não ficarem em inglês. Ele não aparece em inglês nem quando o pacote já está instalado.
- **Apps da pasta de início automático**: os apps de `~/.config/autostart` e `/etc/xdg/autostart` (Nextcloud, Dropbox, um `.desktop` que você pôs ali) agora abrem na sessão do Swaydesk, pelo systemd, e fecham no logout. As entradas do que o Swaydesk já roda, ou que brigam com ele, são puladas só na sessão do Swaydesk: blueman, nm-applet, os agentes do polkit, xapp-sn-watcher, o update-notifier do Ubuntu, o touchegg do Mint (os gestos de 3 e 4 dedos já são do Sway), o `im-launch` do im-config e o pulseaudio quando o servidor de som é o PipeWire. O GNOME, o KDE ou o Cinnamon da mesma conta continuam abrindo-as. Para usar o IBus ou o fcitx5 no Swaydesk, suba-o pelo `~/.config/sway/config.d/50-local.conf` (`exec ibus-daemon -drx` ou `exec fcitx5 -d`); a entrada de autostart do próprio fcitx5 continua valendo. Para desligar, ponha `autostart = false` em `[enabled]` do `~/.config/swaydesk/config.toml` (num arquivo de uma versão anterior a chave não existe, o que vale ligado: acrescente a linha). Para pular um app só, vale a regra de sempre: uma cópia em `~/.config/autostart` com `Hidden=true`. No Linux Mint, isso faz o gerenciador de atualizações do próprio Mint (mintupdate) abrir também na sessão do Swaydesk, e o `update_notifier = false` não o desliga: desmarque-o em Aplicativos de Sessão do Cinnamon (isso o desliga no Cinnamon também; para escondê-lo só no Swaydesk, ponha uma cópia da entrada dele em `~/.config/autostart` com `NotShowIn=sway;`).
- **Arch: a janela de atualização roda o `paku maintain`**: as notícias do Arch, depois a atualização completa (repositórios oficiais, AUR e Flatpak) e então a limpeza: pacotes órfãos, Flatpaks sem uso, o cache de pacotes, arquivos `.pacnew`, a checagem de kernel novo e os serviços a reiniciar. Todo passo que apaga ou muda algo tem Não como padrão. A tela de login e a sua sessão nunca são reiniciadas; quando é preciso reiniciar, uma linha avisa. A atualização do firmware continua sendo um passo à parte, depois dele.
- **As mensagens da tela de login vão para o journal do sistema**: `sudo journalctl -t swaydesk-greeter` (ou sem sudo, no grupo `adm` ou `systemd-journal`), em todas as distribuições. Antes, nada do que ela registrava ficava guardado, e uma sessão que não abria não deixava rastro. Senhas nunca são registradas, e um nome digitado em **Outro usuário…** aparece como `<user>` nos erros registrados.

### Mudou

- **As bordas das janelas têm 3 px em vez de 2**, nas janelas lado a lado e nas flutuantes. Uma janela sozinha na área de trabalho continua sem borda, e a ativa continua se destacando pela cor. Um `default_border` seu no `~/.config/sway/config.d/50-local.conf` continua valendo.
- **Os campos de busca e de senha ficaram iguais**: o campo de senha da tela de bloqueio deixa de ser uma pílula e ganha o estilo das buscas da grade de apps e do histórico da área de transferência (cantos de 8 px, uma borda fina, a cor de destaque no foco), e o campo de busca do lançador (rofi) ganha a mesma borda, sempre na cor de destaque, já que ele está sempre com o foco. Na grade de apps e no histórico, a lupa agora fica à mesma distância do texto que no lançador.
- O `~/.config/sway/config.d/50-local.conf` de uma conta nova agora vem com os comentários no idioma da sessão (inglês, português ou espanhol); antes vinha sempre em português. Um arquivo que já existe nunca é tocado.
- No Debian e no Ubuntu, o `swaydesk-desktop` também traz o pacote novo `swaydesk-efi`, que mantém o caminho de boot reserva do firmware funcionando com o Secure Boot nos sistemas instalados por uma imagem do Swaydesk (veja **Corrigido** e **Atualizando da 0.1.3.x**). Em qualquer outro sistema, inclusive num que só adicionou o repositório do Swaydesk, ele não faz nada: nunca mexe na partição EFI, nas entradas de boot nem nos arquivos de outro sistema. Para desligá-lo: `sudo mkdir -p /etc/swaydesk && echo off | sudo tee /etc/swaydesk/efi-fallback`. Para ver por que ele agiu ou não num sistema, rode `sudo SWAYDESK_EFI_VERBOSE=1 /usr/libexec/swaydesk/swaydesk-efi-fallback`.
- No Debian e no Ubuntu, o pacote novo `swaydesk-dpkg-includes` mantém as traduções do Swaydesk nos sistemas cujo dpkg as descarta (veja **Corrigido**). Ele só acrescenta um filtro do dpkg para os arquivos de tradução do próprio Swaydesk.
- **Arch: o `swaydesk-desktop` depende do paku** (0.3.0 ou mais novo), o wrapper do pacman e do AUR que o Swaydesk usa nas atualizações, vindo do repositório do Swaydesk, e do `archlinux-contrib` (para a checagem dos serviços). O paku usa o paru, o yay ou o pikaur se um deles estiver instalado (ou o indicado em `~/.config/paku/config`), e só o pacman se não houver nenhum. O Swaydesk continua sem instalar nenhum helper do AUR.
- **As pastas de configuração agora se chamam `swaydesk`**: `~/.config/swaydesk`, `~/.local/share/swaydesk` e `~/.local/state/swaydesk` (eram `sway-desktop`), e a pasta da Waybar `~/.config/swaydesk-desktop/waybar` vai para `~/.config/swaydesk/waybar`. Elas mudam no primeiro login depois da atualização, nunca no meio de uma sessão, e os nomes antigos ficam como links: os caminhos antigos continuam funcionando por eles, sem nada a fazer. O cache recomeça em `~/.cache/swaydesk`. Veja **Atualizando da 0.1.3.x**.
- Os READMEs e o site agora dizem com todas as letras: o Swaydesk é um projeto independente feito sobre o [Sway](https://swaywm.org), sem ligação com o projeto Sway nem com o endosso dele ou dos seus desenvolvedores.

### Corrigido

- **A dock no SwayFX 0.6** nunca aparecia: o SwayFX para de redesenhar um painel que está quase todo fora da tela, e a dock começava a subir dali. Agora ela mantém uma pequena parte na tela enquanto desliza, e nada fica na tela depois que ela se esconde.
- **A grade de apps sumia depois do login, ou o Super não fazia nada**, quando outro usuário do mesmo computador estava com a grade ou a tela de bloqueio abertas: os scripts do Swaydesk confundiam os processos dele com os seus. Agora toda busca de processo só enxerga os seus. A grade também reinicia sozinha se parar, como a dock já fazia.
- **O Bluetooth pedia a senha de administrador a cada login** no Debian e no Ubuntu, numa conta fora dos grupos `sudo` e `netdev`: o plugin KillSwitch do blueman agora fica desligado na sessão do Swaydesk. Ligar o Bluetooth pelo popup agora também tira um bloqueio por software (a tecla de modo avião, `rfkill block`) sem pedir senha.
- **O Tema do Desktop perdia mudanças feitas de fora**: com qualquer coisa pendente na janela, uma mudança feita fora dela (`swaydesk-theme set` num terminal, a troca automática entre claro e escuro) era ignorada, e o **Aplicar** a desfazia. Agora os campos que você não mexeu na janela seguem a mudança, e só os editados ficam.
- **O Swaydesk em inglês no Ubuntu minimizado** (as imagens de servidor e de nuvem), cujo dpkg descarta as traduções de todos os pacotes: agora o Swaydesk mantém as suas. As traduções dos outros pacotes, as dos pacotes de idioma inclusive, voltam com o `sudo unminimize`, e num sistema assim o aviso do pacote de idioma mostra o `sudo unminimize` primeiro.
- **Sem boot com o Secure Boot depois de perder a entrada de boot**, nos sistemas Debian 13, Debian testing e Ubuntu instalados pelas imagens 0.1.3.x: o caminho reserva do firmware (`EFI/BOOT/BOOTX64.EFI`) tinha o GRUB, que o Secure Boot recusa, então um reset do firmware ou o disco levado para outro PC deixava o sistema sem boot. Agora o caminho reserva tem o shim assinado, com tudo o que precisa para dar boot e recriar a entrada. Os sistemas Debian 13 e Debian testing recebem a correção na atualização; os Ubuntu, na próxima atualização do GRUB ou do shim (veja **Atualizando da 0.1.3.x**).
- **O nome do layout do teclado** no canto da tela de login e na tela de bloqueio agora vem no idioma do sistema ("Português (Brasil)", "Portugués (Brasil)" em vez de "Portuguese (Brazil)"), pelas traduções do próprio xkeyboard-config. No Ubuntu e no Mint elas vêm com o pacote de idioma (`language-pack-pt-base`, `language-pack-es-base`); sem ele, o nome fica em inglês.
- **A data da tela de login e da tela de bloqueio** segue o mesmo idioma do relógio da sessão: a chave `locale` do `~/.config/swaydesk/config.toml` (na tela de bloqueio) ou do `/etc/swaydesk-greeter/config.toml` (na tela de login), se definida; senão `LC_ALL`, `LC_TIME` e por fim `LANG` (na tela de login, lidos do `/etc/locale.conf` ou do `/etc/default/locale`). A tela de login só lia o `LANG`. Um `LC_TIME=C.UTF-8` posto de propósito continua dando datas no formato ISO. Se o idioma escolhido não estiver gerado no sistema, a data agora usa o idioma dos textos da tela, e não o formato C. No Arch, depois de editar o `/etc/locale.conf` à mão, rode `sudo locale-gen`.
- Arch: o idioma do sistema, do `/etc/locale.conf`, agora é habilitado no `/etc/locale.gen` e gerado quando o `swaydesk-desktop` é instalado ou atualizado. Antes, um idioma não gerado deixava a sessão em inglês.

### Imagens de instalação

- **Imagens só do Debian 13 e do Arch a partir da 0.1.4**: o Ubuntu 26.04 e o Debian testing se instalam pelos pacotes, do repositório do Swaydesk num sistema que você mesmo instala (o Debian testing é de melhor esforço). Os sistemas instalados pelas imagens do Ubuntu e do Debian testing da 0.1.3.x continuam recebendo as atualizações do Swaydesk pelo repositório, inclusive a correção do Secure Boot (veja **Atualizando da 0.1.3.x**).
- **"Swaydesk" no menu de boot**: na imagem do Debian 13, a entrada de boot do firmware e o menu do GRUB se chamam Swaydesk, e os arquivos de boot ficam em `EFI/swaydesk`, em todas as atualizações do GRUB. Ao lado de um Debian ou Ubuntu comum no mesmo disco, cada sistema fica com a sua pasta.
- **Reserva do Secure Boot**: também na imagem do Debian 13, o caminho reserva do firmware tem o shim assinado, então o sistema instalado dá boot com o Secure Boot mesmo depois de perder a entrada de boot, e a entrada volta sozinha. Antes, o caminho reserva tinha o GRUB, que o Secure Boot recusa.
- A imagem do Debian 13 ficou cerca de 8% menor (compressão xz, como a do Debian testing da 0.1.3.x usava); a sessão live leva cerca de 4 segundos a mais para abrir (42 s em vez de 38 s num pendrive).

### Atualizando da 0.1.3.x

- **Debian 13 e Debian testing instalados por uma imagem do Swaydesk**: a correção do Secure Boot chega com o `apt upgrade`, e não há nada a fazer.
- **Ubuntu instalado por uma imagem do Swaydesk**: a correção do Secure Boot chega com a próxima atualização do GRUB ou do shim; para tê-la já, rode `sudo apt install --reinstall grub-efi-amd64-signed shim-signed`. O boot do dia a dia não muda em nenhum caso: só um sistema que perde a entrada de boot precisa da correção. Essa atualização do GRUB passa a entrada de boot para `EFI/swaydesk`, ainda uma só entrada "Swaydesk", e a pasta antiga `EFI/ubuntu` fica sem uso. Dá para apagá-la à mão, mas só se nenhum outro Ubuntu ou Linux Mint estiver instalado no mesmo disco, porque eles usam essa mesma pasta; o `sudo efibootmgr -v` mostra quais entradas apontam para ela.
- **Arch, se você instalou o pacu**: ele agora se chama paku, e o pacu saiu do repositório do Swaydesk. Rode `sudo pacman -Syu paku` e responda `y` para remover o pacu. O `swaydesk-desktop` traz o paku de qualquer jeito.
- **Tema GTK**: as contas que já existem continuam com o tema que tinham, como o `Adwaita-dark`. Para ter a cor de destaque nos apps GTK 3, escolha o **adw-gtk3** no Tema do Desktop.
- **Pastas de configuração**: a sessão aberta durante a atualização continua usando as pastas antigas; elas mudam no próximo login. Se uma pasta antiga e a nova já existirem com um item de mesmo nome, nada é movido: a pasta antiga continua em uso e o log da sessão avisa (`grep kept: ~/.local/state/sway/sway.log`); junte-as à mão para trocar. Voltar para a 0.1.3.x não exige nada, porque ela lê os nomes antigos pelos links. O `~/.cache/sway-desktop` antigo pode ser apagado.
- **Ubuntu instalado pelos pacotes**: para ter os textos do próprio GTK (o seletor de arquivos, os diálogos) no seu idioma, rode o comando do aviso do login, `sudo apt install language-pack-gnome-<idioma>`, como `language-pack-gnome-pt`. Num Ubuntu minimizado, rode antes o `sudo unminimize`.

### Limitações conhecidas

- Com o adw-gtk3 no Debian 13 (GTK 4.18), os apps GTK 4 que não usam a libadwaita (como o `gtk4-widget-factory`) aparecem no estilo claro do adw-gtk3 mesmo com um tema escuro. Os apps de libadwaita (a maioria dos apps do GNOME, como o Editor de Texto) e os apps GTK 3 ficam escuros. No Ubuntu 26.04 e no Arch, todos seguem o estilo escuro.
- Com outro desktop instalado ao lado do Swaydesk, a cor de destaque não chega aos apps GTK, porque os arquivos que ela precisa coloririam esse desktop também.
- No Debian 13, o primeiro login de uma conta nova ainda pede a senha do chaveiro uma vez; isso vem de um bug do gnome-keyring 48, corrigido no Debian 14.
- Com um GNOME ou KDE aberto em outro VT ao mesmo tempo que o Swaydesk, os dois dividem o mesmo ambiente do systemd do usuário: enquanto o Swaydesk estiver aberto, o autostart desse desktop também pula o blueman e o nm-applet, e um app que ele abrir depois do seu login no Swaydesk fecha quando você sai do Swaydesk.
