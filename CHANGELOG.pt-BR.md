# Novidades

O que muda em cada versão do Swaydesk, em português a partir da 0.1.4. As versões anteriores estão no [CHANGELOG.md](CHANGELOG.md), em inglês. Cada versão sai primeiro em pacotes e, logo depois, em imagens de instalação. As atualizações chegam pelo `apt upgrade` ou pelo `pacman -Syu`.

## 0.1.10 — 2026-11-27

Um tema de cursor à sua escolha, com o Bibata como cursor padrão no Debian e no Ubuntu, e uma rodada de correções: uma captura por vez, os apps GTK 4 de terceiros escuros no Debian 13, o popup de rede num PC sem Wi-Fi e os switches que mantêm o visual de um tema Mint-Y. O pacote de transição vazio `swaydesk-scripts` sai, e o site passa para https://swaydesk.org.

### Atualização

- **Da 0.1.8 ou 0.1.9:** `sudo apt upgrade` ou `pacman -Syu`. O `apt-get upgrade` puro deixa os pacotes do Swaydesk sem atualizar, porque a 0.1.10 traz um pacote novo, o `bibata-cursor-theme`. No Arch, o `pacman -Syu` pergunta "Replace swaydesk-scripts with swaydesk-common?": responda sim. No Debian e no Ubuntu, o `swaydesk-scripts` continua instalado, vazio, e você pode removê-lo com `sudo apt remove swaydesk-scripts`.
- **Da 0.1.6 ou 0.1.7:** no Debian e no Ubuntu, atualize com `sudo apt full-upgrade`. O `apt upgrade` lista todos os pacotes do Swaydesk em "Not upgrading" e sai sem erro, porque o `swaydesk-scripts` precisa ser removido. Depois, leia as notas da 0.1.8 e da 0.1.7 abaixo.

### Mudanças

- **Um tema de cursor à sua escolha.** A nova chave `cursor_theme`, no topo do `config.toml`, escolhe qualquer tema de cursor instalado (uma pasta com `cursors/`). O `swaydesk-desktop-config apply` e o app Tema a aplicam na hora: o cursor do Sway, as configurações do GTK, o `gsettings` e o `XCURSOR_THEME` da sessão. Sem a chave, fica o cursor automático: o Bibata Modern Classic no Debian e no Ubuntu, o Capitaine no Arch. Um tema que não está instalado só gera um aviso. Tirar a chave traz de volta o automático, a não ser que você tenha posto o cursor por outro caminho. Uma linha `seat … xcursor_theme` sua em `~/.config/sway/config.d` continua valendo, e o apply avisa.
- **O Bibata no Debian e no Ubuntu.** O `swaydesk-desktop` agora recomenda o `bibata-cursor-theme`, que está no Debian 13, no Debian testing e no Ubuntu 26.04. As instalações novas e as atualizações ficam com o cursor Bibata da sessão em vez do padrão do Xcursor.
- **O `swaydesk-scripts` saiu.** O pacote de transição vazio da 0.1.7 saiu na 0.1.10, antes da 0.2.0 que as notas da 0.1.8 anunciavam. O `swaydesk-common` fica no lugar dele: no Arch, o `pacman -Syu` o substitui; no Debian e no Ubuntu, ele é removido vindo da 0.1.6 ou 0.1.7 e fica como um pacote vazio vindo da 0.1.8 ou 0.1.9.
- **O site agora é https://swaydesk.org;** o endereço antigo redireciona. A página dos pacotes, os links dos snippets e a identidade visual do instalador apontam para lá.

### Correções

- **Uma captura ou gravação por vez.** Um segundo Print enquanto você ainda escolhe a área não faz nada, em vez de abrir outra seleção. A tecla de gravação durante uma gravação só mostra um aviso. Uma captura durante a gravação continua funcionando.
- **Os apps GTK 4 ficam escuros com o adw-gtk3-dark no Debian 13.** O Controle de Volume (pavucontrol) e os outros apps GTK 4 sem libadwaita abriam claros no estilo escuro, com o GTK anterior ao 4.20. A correção está no `adw-gtk-theme` 6.5-2, nos repositórios do Debian e do Ubuntu; no Ubuntu 26.04 e no Arch eles já ficavam escuros.
- **O popup de rede num PC sem placa de Wi-Fi** mostra o Wi-Fi como indisponível ("Nenhuma placa de Wi-Fi") em vez de um switch que não fazia nada. Uma placa conectada depois funciona sem reabrir o popup.
- **Os switches mantêm o visual de um tema Mint-Y.** Com um tema Mint-Y (como o Mint-Y-Dark-Orange), um switch ligado, como o "Usar a cor do papel de parede" do app Tema ou o "Ligada" de Monitores, aparecia com uma caixa quadrada na cor de destaque em volta da pílula do Mint-Y. Agora fica a pílula. Os outros temas continuam com o switch na cor de destaque, e o radio marcado é redondo em qualquer tema.
- **O `swaydesk-desktop-config apply` só sugere reiniciar os componentes instalados.** Num Sway puro com um só app do Swaydesk, não fala mais da grade de apps, da dock, do Waybar e de outros que você não tem.
- **O aviso de atualizações do Ubuntu mostra o ícone na bandeja.** Era um espaço vazio na barra com o Papirus; o `swaydesk-common` traz o ícone `system-software-update-symbolic` que faltava, como reserva.
- **Os avisos da config saem uma vez só.** Uma chave inválida no `config.toml` (por exemplo, um `cursor_theme` errado) saía duas vezes no `swaydesk-desktop-config apply`, e a dock, os popups da barra e os outros componentes a repetiam no log da sessão a cada leitura da config (a dock, a cada reload). Cada aviso agora aparece uma vez por execução do programa; um aviso novo ou diferente continua aparecendo.

## 0.1.9 — 2026-11-26

A cor de destaque chega aos widgets de todos os apps do Swaydesk, as abas do Sway e a tela de login passam a segui-la, e mais uma rodada de correções: o clima segue uma cidade nova na hora, o app Telas desfaz uma configuração que você não confirmou mesmo que o app não abra de novo, e os arquivos que o Swaydesk gera passam a ter um cabeçalho em inglês.

### Atualizando

- **Da 0.1.8: nada a fazer.** `apt upgrade`, o app de Programas ou `pacman -Syu`.
- **Da 0.1.6 ou da 0.1.7:** leia antes as notas da 0.1.8 e da 0.1.7, abaixo.

### Mudou

- **A cor de destaque está nos widgets de todos os apps do Swaydesk.** Os interruptores, os botões principais, as caixas de seleção, os controles deslizantes, as linhas selecionadas e o contorno do foco no app Telas, na dock, nas notificações, na tela de bloqueio e nos outros apps do Swaydesk ficavam no azul do GTK, qualquer que fosse o destaque; agora seguem o destaque do app Tema. Um `~/.config/gtk-4.0/gtk.css` seu continua valendo.
- **O app Tema mostra o estilo em uso na cor de destaque.** O botão Escuro ou Claro selecionado é pintado no destaque, em qualquer tema.
- **As abas do Sway parecem abas.** As barras de título dos grupos em abas e empilhados ficaram mais altas, com o título no centro, em seminegrito e com um contorno fino em cada uma. Elas têm três níveis tirados do destaque: a aba com o foco no destaque, a visível de um grupo sem foco num tom mais escuro (mais claro, com um tema claro), e as outras em cinza, com o texto apagado. As bordas das janelas não mudam. No Sway puro, o `swaydesk-theme-tool` traz o mesmo visual no trecho opcional `50-swaydesk-theme-tool.conf`.
- **A tela de login segue o destaque de cada usuário.** Com uma conta escolhida, o `swaydesk-greeter` mostra a cor que aquele usuário escolheu no app Tema, lida do AccountsService: o `swaydesk-common` instala uma pequena interface de extensão, e o `swaydesk-desktop-config` grava a cor ali sempre que ela muda e a cada login. Cada usuário só muda a própria. Sem o `accountsservice`, ou em "Outro usuário…", a tela fica com a cor do `/etc/swaydesk-greeter/config.toml`. A instalação do `swaydesk-common` reinicia o `accounts-daemon`, que só lê interfaces novas ao subir; a lista de usuários da tela de login continua.
- **Os arquivos que o Swaydesk gera têm um cabeçalho em inglês:** uma linha `Generated by sway-desktop-config: …` no lugar da em português, no `settings.ini` do GTK, no qt5ct/qt6ct, nos trechos do `config.d` do Sway, no `sway-mimeapps.list`, na configuração da Waybar e no `gtk.css` da cor de destaque. O cabeçalho antigo continua reconhecido: os arquivos de versões anteriores são reescritos com o novo, e um arquivo sem nenhum dos dois continua intocado. Depois de voltar para a 0.1.8 ou anterior, essas versões tratam os arquivos em inglês como seus e param de atualizá-los.

### Corrigido

- **O clima segue uma cidade nova na hora.** Depois de mudar o `weather_location` no `config.toml`, o ícone da barra e o popup mostravam o lugar antigo por até 15 minutos. Sem rede, aparece o marcador de espera, e não a cidade antiga.
- **O app Telas desfaz uma configuração que você não confirmou, mesmo que o app não abra de novo.** Na 0.1.8, uma configuração deixada sem confirmar quando o app era morto ou a sessão acabava só era desfeita na próxima abertura do app. Agora o serviço de perfis a desfaz no login, quando uma tela é ligada ou desligada, num reload do Sway e antes de aplicar um perfil.
- **O aviso de atualizações no Arch esquece as que já foram instaladas.** Depois de atualizar e reiniciar, a barra mostrava as atualizações que você tinha acabado de instalar até a próxima verificação. No login, a lista guardada agora é conferida com as versões instaladas, sem rede, e a primeira verificação vem 30 s depois do login, não 5 minutos.
- **Aplicar um tema não acusa mais falha no herdr quando ele não está aberto.** A cor nova é gravada, e o herdr a lê no próximo início; um herdr aberto continua sendo recarregado, e um reload que falha (por exemplo, com o config do herdr quebrado) agora é avisado, mesmo com o herdr saindo com sucesso.
- **Telas e Ajuda ficam escuros com o adw-gtk3-dark no Debian 13,** e as janelas do Swaydesk seguem uma troca de tema enquanto estão abertas: Telas, Ajuda e a própria janela do app Tema ficavam no tema com que abriram.
- **Texto claro nos popups escuros com o adw-gtk3-dark no Debian 13.** Os cartões das notificações, o calendário e as outras superfícies sempre escuras (os popups da barra, a dock, os avisos, a grade de apps, o OSD, o Alt+Tab) mostravam parte do texto e dos ícones em preto sobre o fundo escuro.
- **As dicas de reinício depois do `swaydesk-desktop-config apply` funcionam numa instalação por pacotes.** Elas citavam os scripts dos dotfiles (`~/.local/bin/…`), que a instalação por pacotes não tem. Na sessão do Swaydesk, agora dão o `sway-waybar restart` e um comando ao qual a sessão responde reabrindo a grade de apps e o OSD; no Sway puro, as units dos snippets opcionais.
- **Sem aviso da Waybar no Sway puro sem Waybar.** O `swaydesk-desktop-config generate`, que o app Tema roda, mostrava `waybar config not generated: … No such file or directory` quando não havia nenhuma configuração da Waybar. Agora ele pula a barra sem avisar, e continua avisando quando existe uma configuração da Waybar que não dá para usar.
- **O `swaydesk-common` depende do `hicolor-icon-theme`.** Ele instala os ícones do desktop em `/usr/share/icons/hicolor`, e sem o índice desse tema o GTK não procura lá: num sistema mínimo, os ícones faltavam. No Arch, cada pacote também depende das bibliotecas que usa, como o `glibc` e o `libgcc`.

## 0.1.8 — 2026-11-26

Um clique no aviso de captura ou de gravação abre o arquivo, e as imagens de instalação abrem fotos no Loupe e PDFs no Papers. Os apps abertos pela dock e pela grade de apps agora rodam no seu próprio escopo do systemd, todo programa do Swaydesk responde a `--help` e `--version`, e os campos de busca vêm em três tamanhos.

### Atualizando

- **Da 0.1.7: nada a fazer.** `apt upgrade`, o app de Programas ou `pacman -Syu`.
- **Da 0.1.6:** leia antes as notas da 0.1.7, abaixo. No Debian e no Ubuntu, atualize com o `apt upgrade` ou o `apt full-upgrade`, não com o `apt-get upgrade`; no Arch, com um `pacman -Syu` completo.
- **O `swaydesk-scripts` não segura mais nada.** Este pacote de transição, vazio, agora aceita qualquer versão do `swaydesk-common`: quando ele sair do repositório na 0.2.0, uma cópia que ficou instalada não prende os outros pacotes na 0.1.8. Você pode removê-lo quando quiser; removê-lo não desinstala nada.

### Novo

- **Um clique no aviso de captura ou de gravação abre o arquivo** no app padrão, no aviso ou depois, na central de notificações. Os avisos agora vêm do "Swaydesk", não do "notify-send". Para isso, no Debian e no Ubuntu o `swaydesk-capture` recomenda o `xdg-utils` e a `libglib2.0-bin` (o gdbus); no Arch, o `xdg-utils` é uma dependência opcional.
- **Imagens de instalação: Loupe e Papers** são os visualizadores padrão de imagens e de PDF numa instalação nova pelas imagens do Debian 13 e do Arch. Os pacotes não mudam, e o app que você escolhe no "Abrir com" ou com o `xdg-mime default` continua vencendo.
- **Cada app aberto pela dock ou pela grade de apps ganha seu próprio escopo do systemd** (`app-swaydesk-….scope` no `app.slice`), como no GNOME e no KDE. Reiniciar a dock ou a grade não lista mais os apps delas como processos que sobraram, e os apps continuam abertos.
- **`--help` e `--version` em todo programa do Swaydesk**, sem tela: o `--version` mostra o programa e a versão do pacote, e o `--help`, o uso. Antes, nenhum tinha `--version`, e a maioria abria uma janela ou esperava a tela.
- **O portal da cor de destaque no Sway puro.** O `swaydesk-portal` traz um `sway-portals.conf` opcional em `/usr/share/swaydesk/xdg-desktop-portal/`: copie-o para `~/.config/xdg-desktop-portal/` (ele diz o que precisa) e os apps que pedem a cor de destaque ao portal recebem a do Swaydesk. O compartilhamento de tela, as capturas e o bloqueio de inatividade continuam funcionando como com o arquivo do próprio Sway. Nada liga sozinho; na sessão do Swaydesk, nada muda.

### Mudou

- **Os campos de busca vêm em três tamanhos:** grande na grade de apps, normal no app Tema do Desktop e na Ajuda, e compacto no popup da área de transferência. A 0.1.7 deu a todos o tamanho do lançador, grande demais fora dele.
- **O bloco do teclado do sistema** no `~/.config/sway/config.d/50-local.conf` tem um cabeçalho na língua da sua sessão que diz o que ele faz: segue o teclado do sistema a cada login até você editá-lo. Um bloco que já existe ganha o cabeçalho novo no próximo login.
- **As descrições dos pacotes e as páginas de manual** agora dizem o que os pacotes fazem: o `swaydesk-portal` dá só a cor de destaque, o `swaydesk-network` pede a senha do Wi-Fi ele mesmo só com o iwd, o `swaydesk-keyring` só age onde o gnome-keyring precisa (Debian 13), e o `swaydesk-desktop-config(1)` documenta o comando `keys`.

### Corrigido

- **Um `true` para um módulo da barra** no seu `config.toml` (`[enabled.bar] weather = true`) agora vence um `false` no `/etc/swaydesk/config.toml` do sistema. O módulo ficava escondido.
- **A Ajuda mostra "Mostrar esta ajuda" uma vez só**, com Super+F1 e Super+/. Eram duas linhas.
- **O Telas desfaz um layout que você não confirmou** quando o app some enquanto pergunta "Manter estas configurações das telas?": fechado, encerrado ou morto (aí, na próxima vez que abrir). O layout não confirmado, que pode ser uma tela preta ou a escala errada, ficava na tela.

## 0.1.7 — 2026-11-24

Cada componente do Swaydesk agora é um pacote próprio: dá para instalar só os que você quer no Sway puro, e iniciá-los com um serviço de usuário ou um trecho do Sway. Também: um campo de busca só em todo o desktop, no modelo do lançador, e, no Arch, o ç no AltGr+, no console. O Tema do Desktop também escolhe o tamanho do cursor, e o `swaydesk-lock` bloqueia a tela sozinho.

### Atualizando da 0.1.6

- **Debian e Ubuntu: atualize com o `apt upgrade`, o `apt full-upgrade` ou o app de Programas.** Esta versão traz pacotes novos, e o `apt-get upgrade` nunca instala pacote novo: com ele, os pacotes do Swaydesk ficariam na 0.1.6.
- **Debian e Ubuntu: atualize pelo repositório, não com alguns `.deb` da 0.1.7.** Instalar só alguns dos pacotes novos sobre a 0.1.6 (por exemplo, o `swaydesk-common` e o `swaydesk-caffeine` de arquivos locais) faz o apt remover o `swaydesk-widgets` e o `swaydesk-scripts` da 0.1.6, cujos arquivos os novos tomam; ele os lista em "REMOVED" antes de perguntar. Atualizar todos os pacotes pelo repositório não remove nada.
- **Numa instalação completa, nada a fazer.** O `swaydesk-desktop` traz todos os componentes, como antes. O `swaydesk-widgets`, o `swaydesk-theme` e o `swaydesk-session` viram metapacotes vazios que trazem os mesmos componentes, e a atualização mantém tudo.
- **Se você chama o `sway-lock`, o `sway-shot` ou o `sway-rec` no seu próprio config do Sway**, sem o desktop completo, instale o `swaydesk-lock` ou o `swaydesk-capture`; para o `sway-osd` (as teclas de volume e brilho), o `swaydesk-osd`. Cada script auxiliar, e cada ícone da barra, agora vai no pacote do componente que o usa. Atualizar um sistema que tinha só um componente (a grade de apps, por exemplo) mantém os scripts dele e tira os que ele nunca usou.
- **O `swaydesk-scripts` agora é um pacote de transição, vazio, e sai na 0.2.0.** Você já pode removê-lo. Removê-lo não desinstala nada. Se você o instalou sozinho, instale os componentes cujos scripts você usa: `swaydesk-lock`, `swaydesk-capture`, `swaydesk-drawer`, `swaydesk-caffeine` ou `swaydesk-osd`.
- **O `swaydesk-lock` sozinho não bloqueia a tela por inatividade.** No Sway puro (ou em outra sessão), ligue-o a uma tecla e deixe o seu próprio swayidle chamá-lo, por exemplo:
  `bindsym Ctrl+Alt+l exec swaydesk-lock`
  `exec swayidle -w timeout 600 swaydesk-lock before-sleep swaydesk-lock`
  Ou inclua o trecho do bloqueio, que tem as duas linhas: `include /usr/share/swaydesk/sway/config.d/50-swaydesk-lock.conf`. Na sessão do Swaydesk, nada muda.
- **Arch: atualize com um `pacman -Syu` completo.** Nesta versão, arquivos passam de um pacote do Swaydesk para outro, e um pacote só da 0.1.7 instalado sobre a 0.1.6 (`pacman -S swaydesk-dock`) para com "conflicting files".
- **Arch: o `swaydesk-desktop` agora pede o paku 0.3.2 ou mais novo.** O paku agora traz o `archlinux-contrib` por conta própria. Se você segura o paku no 0.3.0 ou no 0.3.1 (`IgnorePkg`), deixe-o atualizar antes. Senão, o `swaydesk-desktop` não atualiza.

### Novo

- **Um pacote por componente.**
  - **Itens da barra:** `swaydesk-calendar`, `-weather`, `-audio`, `-network`, `-bluetooth`, `-battery`, `-brightness`, `-caffeine`, `-system`, `-media`, `-clipboard`, `-osd` (o indicador de volume e brilho) e `-language`.
  - **Troca de janelas:** `swaydesk-switcher`.
  - **Apps:** `swaydesk-theme-app` (Tema do Desktop), `-displays`, `-help` e `-capture`.
  - **Sessão:** `swaydesk-session-menu`, `-lock`, `-keyring` e `-sessionmgr`.
  - **Ferramentas de tema:** `swaydesk-theme-tool` e `swaydesk-portal`.
  - **Grupos:** o `swaydesk-bar` traz todos os itens da barra; o `swaydesk-apps` traz os quatro apps.
  - Cada pacote traz só o que precisa. A grade de apps sozinha não puxa mais um gerenciador de arquivos (ela só o sugere) nem o Python. Só o calendário traz o Python, para os feriados; no Debian e no Ubuntu, também a versão GTK 3 da tela de bloqueio. O painel de captura (`swaydesk-capture`) agora depende do grim, do slurp e do jq, que toda captura usa.
- **A dock instalada sozinha começa com o gerenciador de arquivos e um terminal**, em vez de vazia. Ela não grava o arquivo dos fixados até você mudá-los, e um terminal instalado depois aparece sozinho.
- **Serviços de usuário para os componentes que ficam rodando**, para quem instala um sozinho em outra sessão (Sway puro, niri, Hyprland com o uwsm): `swaydesk-dock`, `-drawer` (a grade de apps), `-notifd`, `-osd`, `-switcher`, `-displays` e `-clipboard`, cada um um `.service`, iniciado com `systemctl --user enable --now swaydesk-dock.service`. Nenhum pacote os ativa, e eles nunca rodam na sessão do Swaydesk. Ao lado de uma cópia já rodando, a dock, a grade, a troca de janelas, as telas e o indicador saem sem barulho; o serviço de notificações espera o outro sair; na área de transferência, use o serviço ou as suas próprias linhas do `wl-paste`, não os dois. Os apps abertos pela dock, pela grade ou por uma notificação continuam abertos quando o serviço para ou reinicia. No Sway puro, é mais simples incluir o trecho do componente (veja abaixo).
- **Trechos opcionais para o Sway puro.** Cada um desses componentes, e o bloqueio, traz um `/usr/share/swaydesk/sway/config.d/50-<pacote>.conf`: acrescente `include /usr/share/swaydesk/sway/config.d/50-swaydesk-dock.conf` (por exemplo) ao config do Sway, e ele inicia o serviço do componente com este Sway, acrescenta as teclas dele e o para quando o Sway sai (o do bloqueio inicia o seu swayidle). Os itens da barra trazem um módulo do Waybar para colar, `/usr/share/swaydesk/waybar/<pacote>.jsonc`. Nada liga sozinho, e na sessão do Swaydesk os trechos não fazem nada.
- **A cafeína fora da sessão do Swaydesk não mexe mais no seu swayidle.** Antes, ela o trocava por um sem tempos, e desligar a cafeína deixava o nosso no lugar até o próximo login. Agora ela segura um inibidor de inatividade (o novo `swaydesk-caffeinate`, no `swaydesk-caffeine`), que o swayidle e os outros gerenciadores de inatividade respeitam. Na sessão do Swaydesk, nada muda.
- **O `swaydesk-lock` sem argumentos bloqueia a tela**, e assim funciona sozinho: num atalho e como o `timeout` e o `before-sleep` do swayidle. Ele volta assim que a tela fica bloqueada, ou na hora se ela já estava, como o `swaylock -f`, e não segura a suspensão. O `swaydesk-lock --help` mostra o uso.
- **Tamanho do cursor.** O Tema do Desktop tem uma lista **Tamanho do cursor**, abaixo dos ícones, só com os tamanhos que o tema do cursor tem; a escolha vale na hora. No terminal: `swaydesk-theme cursor` lista os tamanhos, e `swaydesk-theme cursor N` (de 16 a 96) escolhe um; no `config.toml`, é o `cursor_size`. Os apps GTK 4 acompanham na hora, e os Qt 6 no Wayland também, embora alguns mostrem a seta simples até serem reabertos; os GTK 3, Qt 5, Electron e X11, quando reabertos. O cursor do próprio Sway acompanha com qualquer tema de cursor, e uma linha de seat sua no `50-local.conf` continua valendo depois de recarregar.

### Mudanças

- **Todo campo de busca segue o do lançador**: a área de transferência, a grade de apps, o app Tema do Desktop e a Ajuda. A mesma altura, cantos arredondados, a lupa na cor de destaque e um só texto, "Buscar…". Em todos eles, o primeiro Esc limpa o texto e o segundo fecha. Até agora, o Esc fechava o histórico da área de transferência na hora.
- **Arch: no console (TTY), o layout Inglês (EUA, internacional, com teclas mortas) agora tem o ç no AltGr+,**, como na sessão, e uma senha com ç também funciona ali. Uma instalação nova já vem assim. Numa existente, rode uma vez `sudo localectl set-x11-keymap us "" intl` (o pacote traz o mapa do console; o comando só passa a usá-lo).
- **Um componente que não acha um dos seus scripts auxiliares agora avisa**, uma vez, na saída de erro: qual script, onde procurou e qual pacote instalar.
- **O Tema do Desktop sem papel de parede:** sem o arquivo do papel de parede (numa instalação avulsa, por exemplo), o **Usar a cor do papel de parede** começa desligado, em vez de avisar a cada login. Quando houver um papel de parede, ele liga sozinho, a menos que você o tenha escolhido. Sem nenhum tema GTK instalado, a lista agora diz isso e sugere o gnome-themes-extra, em vez de "Nenhum tema encontrado".
- **Uma captura ou gravação sem um programa diz qual falta** ("Falta instalar: wf-recorder."), em vez de "Não foi possível descobrir a tela em foco."
- **O `--help`** do `swaydesk-session-menu` e do `swaydesk-lock` mostra o uso e sai, em vez de abrir o menu ou bloquear.
- **O serviço de notificações, com outro já rodando** (o mako, por exemplo), agora diz na saída de erro qual programa tem o nome. Ele continua esperando e assume quando o outro sai.

### Correções

- **A cor de destaque automática** (Tema do Desktop, **Usar a cor do papel de parede**) não achava o papel de parede quando o `XDG_DATA_HOME` apontava para outro lugar que não o `~/.local/share`. Agora ela lê o mesmo arquivo que o papel de parede e a tela de bloqueio usam.
- **O ícone do monitor do sistema na barra** agora se atualiza quando o popup abre e fecha. Antes, o popup apagava o arquivo de sinal do ícone em vez de sinalizá-lo.
- **Iniciado pelo nome do pacote** (`exec swaydesk-osd-bar` no Sway puro), o indicador de volume e brilho nunca aparecia: o volume mudava, mas a barra não. Ele, e todo popup da barra, agora roda com o nome que as teclas e os ícones procuram.

## 0.1.6.1 — 2026-11-24

Só imagens de instalação: as imagens do Debian 13 e do Arch da 0.1.6. Os pacotes continuam na 0.1.6.

### Imagens de instalação

- **O instalador exige uma senha de pelo menos 4 caracteres para o primeiro usuário.**
- Elas também trazem as mudanças de imagem listadas na 0.1.6: nada de `/etc/calamares` no sistema instalado, no Debian 13 e no Arch; no Arch, formatos em português com um fuso no Brasil, a página de teclado começando no layout US e um menu do GRUB sem todas as entradas de boot do firmware.

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
