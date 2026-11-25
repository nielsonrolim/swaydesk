//! A tela de login do Sway Desktop: GTK 4 com o gtk4-layer-shell, num Sway próprio que o greetd abre
//! (`/usr/share/swaydesk-greeter/sway.conf`). A conversa com o greetd é a do
//! [`swaydesk_greeter_logic::ipc`]; o visual é o da tela de bloqueio.
//!
//! O fluxo: escolher a conta (ou digitar outra), a senha e Enter. A senha responde à primeira pergunta
//! secreta do PAM; outras perguntas (um código, a troca de senha vencida) aparecem uma a uma no campo. Com
//! o sucesso, a sessão escolhida é aberta, a memória é gravada e o programa sai: o greetd fecha o Sway da
//! tela e abre a sessão.
//!
//! Segurança:
//! - a conversa com o greetd (e o PAM atrás dele, que pode demorar) roda fora da linha da interface;
//! - a senha sai do campo para um Vec zerado depois de enviado; o campo é limpo na hora;
//! - o texto de fora (nomes do GECOS, mensagens do PAM, nomes das sessões) só entra com set_text, nunca
//!   como markup, e com largura limitada;
//! - a memória é só uma dica: a sessão é procurada na lista das instaladas, e o último usuário só vale se
//!   estiver na lista ou passar no `valid_username`.
//!
//! Ambiente: GREETD_SOCK (do greetd). Para testes: SWAYDESK_GREETER_CONFIG e SWAYDESK_GREETER_STATE trocam
//! os arquivos da config e da memória.
use std::cell::{Cell, RefCell};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gdk, gdk_pixbuf, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use swaydesk_common::config;
use swaydesk_common::i18n::tr;
use swaydesk_common::keyboard;
use swaydesk_greeter_logic::ipc::{Client, Step};
use swaydesk_greeter_logic::sessions::{self, Session};
use swaydesk_greeter_logic::state::{State, STATE_FILE};
use swaydesk_greeter_logic::system;
use swaydesk_greeter_logic::users;
use swaydesk_lock_logic as sl;

const AVATAR: i32 = 96;
const PHOTO_MAX_BYTES: u64 = 2 * 1024 * 1024;
const PHOTO_MAX_SIDE: i32 = 2048;
const SHAKE_MS: f64 = 400.0;
/// O PAM pode demorar (LDAP, leitor de digital, a espera do pam_faillock); passado isso, o campo volta
const GREETD_TIMEOUT: Duration = Duration::from_secs(120);
const SWAYMSG_TIMEOUT: Duration = Duration::from_secs(2);
const LAYOUT_EVERY_S: u32 = 2;
const NAME_CHARS: i32 = 32;
// Nerd Font: suspender, reiniciar e desligar
const ICON_SUSPEND: &str = "\u{f04b2}";
const ICON_REBOOT: &str = "\u{f0709}";
const ICON_POWEROFF: &str = "\u{f0425}";

// O cartão e os campos são os do nwg-hello.css da tela anterior; a hora, o avatar e o tremor, os do
// bloqueio. O gradiente usa o destaque e dispensa uma imagem de fundo
const CSS: &str = r#"
window.greeter {
  background-color: #1b1b1f;
  background-image: linear-gradient(160deg, alpha(@accent, 0.22), alpha(@accent, 0.02) 55%, alpha(@accent, 0.08));
  color: #dadada;
}
.time { color: #f2f2f2; font-size: 96px; font-weight: 300; }
.date { color: #b8b8bd; font-size: 20px; }
.card {
  background-color: rgba(46, 46, 51, 0.88); border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 18px; padding: 28px 30px 24px 30px; box-shadow: 0 12px 40px rgba(0, 0, 0, 0.45);
}
.avatar { min-width: 96px; min-height: 96px; border-radius: 48px; background: @accent; }
.initials { color: @on_accent; font-size: 34px; font-weight: bold; }
.name { color: #ffffff; font-size: 18px; font-weight: bold; }
.prompt { font-size: 13px; color: #8a8a8f; }
dropdown > button, entry.field {
  min-height: 38px; border-radius: 10px; border: 1px solid rgba(255, 255, 255, 0.08);
  background-image: none; background-color: #38383d; color: #dadada; box-shadow: none; padding: 0 12px;
}
dropdown > button { font-size: 14px; }
dropdown > button:hover { background-color: #424247; }
entry.field { font-size: 16px; caret-color: @accent; outline: none; }
entry.field:focus-within { border-color: @accent; box-shadow: 0 0 0 2px alpha(@accent, 0.35); }
.status { color: #f2c94c; font-size: 13px; min-height: 18px; }
.status.error { color: #ff8a80; }
button.login {
  min-height: 40px; border-radius: 10px; border: none; background-image: none; background-color: @accent;
  color: @on_accent; font-size: 15px; font-weight: bold; box-shadow: none;
}
button.login:hover { background-color: @accent_hover; }
button.flat-btn {
  background: transparent; border: none; box-shadow: none; color: #8a8a8f; padding: 6px 10px;
  border-radius: 12px; min-height: 0;
}
button.flat-btn:hover { background-color: rgba(255, 255, 255, 0.06); color: @accent_hover; }
.glyph { font-family: "Symbols Nerd Font"; font-size: 22px; }
.corner { color: #dadada; font-size: 14px; }
"#;

fn log(message: &str) {
    eprintln!("swaydesk-greeter: {message}");
}

/// Zera a memória com escritas que o compilador não tira.
fn zero(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
}

fn label(class: &str) -> gtk4::Label {
    let label = gtk4::Label::new(None);
    label.add_css_class(class);
    label
}

fn flat_button(text: &str, tooltip: &str) -> gtk4::Button {
    let button = gtk4::Button::with_label(text);
    button.add_css_class("flat-btn");
    button.add_css_class("glyph");
    button.set_tooltip_text(Some(tooltip));
    button.set_focus_on_click(false);
    button
}

/// O filho começa sem sinais bloqueados: as linhas do GIO (o spawn_blocking) bloqueiam todos, e o
/// Command da biblioteca padrão passa a máscara adiante (veja o swaydesk-lock).
fn unblocked(command: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt;
    // SAFETY: só chamadas async-signal-safe entre o fork e o exec
    unsafe {
        command.pre_exec(|| {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
            Ok(())
        })
    }
}

/// A saída de um comando que termina com 0 dentro do prazo; senão None, e ele é morto.
fn output_within(command: &mut Command, timeout: Duration) -> Option<Vec<u8>> {
    let mut child = unblocked(command).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut data);
        data
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let status = status?;
    let data = reader.join().ok()?;
    status.success().then_some(data)
}

/// A pasta que faz de XDG_CONFIG_HOME da tela no US internacional: só o gtk-4.0/Compose (install-tree.sh).
/// Só leitura: a tela não grava configurações (GSettings, settings.ini); se um dia gravar, não pode ser aqui.
const INTL_CONFIG_HOME: &str = "/usr/share/swaydesk-greeter/intl";

/// Aplica o teclado do sistema no Sway da tela. Bloqueia: numa linha à parte.
fn apply_system_keyboard() {
    let Some((_, text)) = keyboard::system_keyboard(Path::new("/etc/default/keyboard")) else { return };
    let Some(command) = system::keyboard_command(&keyboard::xkb_settings(&text)) else { return };
    if output_within(Command::new("swaymsg").arg(&command), SWAYMSG_TIMEOUT).is_none() {
        log(&format!("could not apply the keyboard: {command}"));
    }
}

/// O cursor da sessão (o Bibata, senão o Capitaine), o mesmo da sessão do pacote. Sem nenhum, o padrão.
/// Bloqueia: numa linha à parte.
fn apply_cursor() {
    let Some(theme) = swaydesk_common::cursor::cursor_theme() else { return };
    let size = swaydesk_common::cursor::CURSOR_SIZE.to_string();
    if output_within(Command::new("swaymsg").args(["seat", "seat0", "xcursor_theme", theme, &size]), SWAYMSG_TIMEOUT).is_none() {
        log(&format!("could not apply the cursor {theme}"));
    }
}

/// O layout ativo e se há outro para trocar. Bloqueia.
fn read_layout() -> (String, bool) {
    output_within(Command::new("swaymsg").args(["-r", "-t", "get_inputs"]), SWAYMSG_TIMEOUT)
        .and_then(|out| serde_json::from_slice(&out).ok())
        .map(|inputs| sl::keyboard_layout(&inputs))
        .map(|(name, many)| (swaydesk_common::i18n::layout_name(&name), many))
        .unwrap_or_default()
}

/// A foto decodificada: os pixels RGB(A), a largura, a altura, o rowstride e se tem alfa.
type Photo = (glib::Bytes, i32, i32, usize, bool);

/// A foto da conta, só a do AccountsService (/var/lib/AccountsService/icons/<login>, do root, como no GDM):
/// nunca o ~/.face, que é da própria conta e seria decodificado pelo processo que vê as senhas de todos.
/// Mesmo assim, com cuidado: aberto sem seguir link nem bloquear (um FIFO), só arquivo comum de até 2 MiB,
/// só PNG ou JPEG pelos bytes do começo, até 2048x2048, reduzido ao tamanho do avatar. Roda numa linha à
/// parte; o Pixbuf não atravessa linhas, os pixels sim.
fn decode_photo(path: &Path) -> Option<Photo> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > PHOTO_MAX_BYTES {
        return None;
    }
    let mut data = Vec::new();
    file.take(PHOTO_MAX_BYTES + 1).read_to_end(&mut data).ok()?;
    // O tamanho pelo cabeçalho, antes do decodificador: o size-prepared do gdk-pixbuf chega tarde (o PNG já
    // foi alocado inteiro); ele fica como segunda barreira
    let (width, height) = image_size(&data)?;
    if width == 0 || height == 0 || width > PHOTO_MAX_SIDE as u32 || height > PHOTO_MAX_SIDE as u32 {
        return None;
    }
    let kind = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        "jpeg"
    } else {
        return None;
    };
    let loader = gdk_pixbuf::PixbufLoader::with_type(kind).ok()?;
    let refused = Rc::new(Cell::new(false));
    let flag = refused.clone();
    loader.connect_size_prepared(move |loader, width, height| {
        if width <= 0 || height <= 0 || width > PHOTO_MAX_SIDE || height > PHOTO_MAX_SIDE {
            flag.set(true);
            loader.set_size(1, 1);
            return;
        }
        // Cobre o círculo do avatar: o lado menor fica com o tamanho dele
        let scale = AVATAR as f64 / width.min(height) as f64;
        if scale < 1.0 {
            loader.set_size(((width as f64 * scale).round() as i32).max(1), ((height as f64 * scale).round() as i32).max(1));
        }
    });
    let written = loader.write(&data);
    let closed = loader.close();
    if refused.get() || written.is_err() || closed.is_err() {
        return None;
    }
    let pixbuf = loader.pixbuf()?;
    // O quadrado do meio, para o círculo do avatar
    let side = pixbuf.width().min(pixbuf.height());
    let pixbuf = pixbuf.new_subpixbuf((pixbuf.width() - side) / 2, (pixbuf.height() - side) / 2, side, side).copy()?;
    Some((pixbuf.read_pixel_bytes(), pixbuf.width(), pixbuf.height(), pixbuf.rowstride() as usize, pixbuf.has_alpha()))
}

/// A largura e a altura do cabeçalho de um PNG (o IHDR) ou de um JPEG (o primeiro SOFn), sem decodificar:
/// uma imagem pequena no disco pode declarar 20000x20000 e ocupar gigabytes ao abrir.
fn image_size(data: &[u8]) -> Option<(u32, u32)> {
    let be16 = |at: usize| data.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as u32);
    let be32 = |at: usize| data.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return (data.get(12..16) == Some(b"IHDR")).then(|| Some((be32(16)?, be32(20)?))).flatten();
    }
    if !data.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut at = 2;
    while at + 4 <= data.len() {
        if data[at] != 0xff {
            return None;
        }
        let marker = data[at + 1];
        // Enchimento (0xff repetido) e marcadores sem tamanho
        if marker == 0xff {
            at += 1;
            continue;
        }
        if marker == 0xd8 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            at += 2;
            continue;
        }
        let len = be16(at + 2)? as usize;
        if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
            return Some((be16(at + 7)?, be16(at + 5)?));
        }
        if len < 2 {
            return None;
        }
        at += 2 + len;
    }
    None
}

fn photo_texture(photo: Photo) -> gdk::Texture {
    let (bytes, width, height, stride, alpha) = photo;
    let format = if alpha { gdk::MemoryFormat::R8g8b8a8 } else { gdk::MemoryFormat::R8g8b8 };
    gdk::MemoryTexture::new(width, height, format, &bytes, stride).upcast()
}

/// Um texto de fora (nome do GECOS, nome de sessão) cortado para caber na lista.
fn short(text: &str) -> String {
    if text.chars().count() <= NAME_CHARS as usize {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(NAME_CHARS as usize - 1).collect();
    cut.push('…');
    cut
}

fn connect() -> std::io::Result<Client<UnixStream>> {
    let path = std::env::var_os("GREETD_SOCK").filter(|p| !p.is_empty()).ok_or_else(|| std::io::Error::other("GREETD_SOCK is not set"))?;
    let stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(GREETD_TIMEOUT))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    Ok(Client::new(stream))
}

/// O resultado de uma ida ao greetd.
enum Outcome {
    /// A sessão foi aberta: sair
    Started,
    /// Uma pergunta para a pessoa responder
    Prompt { secret: bool, text: String, notes: Vec<String> },
    Failed { auth: bool, text: String, notes: Vec<String> },
    /// O greetd não respondeu ou fechou o socket: a conexão é refeita na próxima vez
    Broken(String),
}

/// Segue a conversa a partir de um passo. Com `password`, ela responde à primeira pergunta secreta; com
/// o sucesso, abre a sessão. Bloqueia.
fn drive(client: &mut Client<UnixStream>, mut step: Step, mut password: Option<Vec<u8>>, session: &Session) -> Outcome {
    let mut all_notes = Vec::new();
    let outcome = loop {
        match step {
            Step::Prompt { secret: true, notes, .. } if password.is_some() => {
                all_notes.extend(notes);
                let mut secret = password.take().unwrap_or_default();
                // O texto veio de um GString, sempre UTF-8; sem cópia, para só haver um buffer a zerar
                let reply = client.respond(std::str::from_utf8(&secret).unwrap_or(""));
                zero(&mut secret);
                match reply {
                    Ok(next) => step = next,
                    Err(err) => break Outcome::Broken(err.to_string()),
                }
            }
            Step::Prompt { secret, text, notes } => {
                all_notes.extend(notes);
                break Outcome::Prompt { secret, text, notes: all_notes };
            }
            Step::Failed { auth, text, notes } => {
                all_notes.extend(notes);
                break Outcome::Failed { auth, text, notes: all_notes };
            }
            Step::Ready { notes } => {
                all_notes.extend(notes);
                break match client.start(&session.cmd, &session.env()) {
                    Ok(()) => Outcome::Started,
                    Err(text) => {
                        let _ = client.cancel();
                        Outcome::Failed { auth: false, text, notes: all_notes }
                    }
                };
            }
        }
    };
    if let Some(mut secret) = password {
        zero(&mut secret);
    }
    outcome
}

/// A tela de um monitor. Só a principal tem o formulário; as outras, a hora e a data.
struct Screen {
    window: gtk4::Window,
    time: gtk4::Label,
    date: gtk4::Label,
}

/// O formulário, na tela principal.
struct Form {
    users: gtk4::DropDown,
    other: gtk4::Entry,
    photo: gtk4::Image,
    letters: gtk4::Label,
    name: gtk4::Label,
    prompt: gtk4::Label,
    password: gtk4::PasswordEntry,
    answer: gtk4::Entry,
    sessions: gtk4::DropDown,
    login: gtk4::Button,
    status: gtk4::Label,
    layout: gtk4::Button,
}

struct App {
    main_loop: glib::MainLoop,
    display: gdk::Display,
    users: Vec<sl::User>,
    sessions: Vec<Session>,
    state: RefCell<State>,
    state_file: PathBuf,
    time_locale: String,
    screens: RefCell<Vec<Screen>>,
    form: RefCell<Option<Rc<Form>>>,
    client: RefCell<Option<Client<UnixStream>>>,
    busy: Cell<bool>,
    photo_generation: Cell<u64>,
    /// Uma pergunta do PAM está na tela (não a da senha do começo)
    prompting: Cell<bool>,
    prompt_secret: Cell<bool>,
}

impl App {
    /// O usuário escolhido: o da lista, ou o digitado em "Outro usuário".
    fn chosen_user(&self, form: &Form) -> Option<String> {
        let index = form.users.selected() as usize;
        match self.users.get(index) {
            Some(user) => Some(user.name.clone()),
            None => Some(EditableExt::text(&form.other).trim().to_string()).filter(|n| users::valid_username(n)),
        }
    }

    fn chosen_session(&self, form: &Form) -> Option<Session> {
        self.sessions.get(form.sessions.selected() as usize).cloned()
    }

    /// Mostra a foto, as iniciais e o nome da conta escolhida, e a sessão dela.
    fn show_user(self: &Rc<Self>, form: &Form) {
        let index = form.users.selected() as usize;
        let accounts = Path::new(sl::ACCOUNTS);
        let user = self.users.get(index);
        form.other.set_visible(user.is_none());
        form.name.set_visible(user.is_some());
        let name = user.map(|user| sl::real_name(user, accounts)).unwrap_or_default();
        form.name.set_text(&name);
        let initials = sl::initials(&name);
        form.letters.set_text(&initials);
        form.letters.set_visible(!initials.is_empty());
        form.photo.set_paintable(None::<&gdk::Paintable>);
        form.photo.set_visible(false);
        // A foto chega depois, de uma linha à parte; uma troca de conta no meio descarta a velha
        let generation = self.photo_generation.get() + 1;
        self.photo_generation.set(generation);
        if let Some(user) = user {
            let path = accounts.join("icons").join(&user.name);
            let app = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let photo = gio::spawn_blocking(move || decode_photo(&path)).await.ok().flatten();
                let Some(app) = app.upgrade() else { return };
                let form = app.form.borrow().clone();
                if let (Some(photo), Some(form)) = (photo, form) {
                    if app.photo_generation.get() == generation {
                        form.photo.set_paintable(Some(&photo_texture(photo)));
                        form.photo.set_visible(true);
                        form.letters.set_visible(false);
                    }
                }
            });
        }
        // A sessão lembrada, se ainda estiver instalada; senão, a primeira (a nossa)
        let remembered = user.and_then(|u| self.state.borrow().sessions.get(&u.name).cloned());
        let position = remembered.and_then(|id| self.sessions.iter().position(|s| s.id == id)).unwrap_or(0);
        form.sessions.set_selected(position as u32);
        self.focus_field(form);
    }

    fn focus_field(&self, form: &Form) {
        if WidgetExt::is_visible(&form.other) && EditableExt::text(&form.other).is_empty() {
            form.other.grab_focus();
        } else if WidgetExt::is_visible(&form.answer) {
            form.answer.grab_focus();
        } else {
            form.password.grab_focus();
        }
    }

    fn set_status(&self, form: &Form, text: &str, error: bool) {
        form.status.set_text(text);
        if error {
            form.status.add_css_class("error");
        } else {
            form.status.remove_css_class("error");
        }
    }

    fn caps_lock(&self) -> bool {
        self.display.default_seat().and_then(|seat| seat.keyboard()).is_some_and(|keyboard| keyboard.is_caps_locked())
    }

    fn show_caps(&self) {
        let Some(form) = self.form.borrow().clone() else { return };
        if !self.busy.get() && !form.status.has_css_class("error") {
            self.set_status(&form, &if self.caps_lock() { tr("Caps Lock is on") } else { String::new() }, false);
        }
    }

    fn set_busy(&self, form: &Form, busy: bool) {
        self.busy.set(busy);
        for widget in [
            form.users.upcast_ref::<gtk4::Widget>(),
            form.other.upcast_ref(),
            form.password.upcast_ref(),
            form.answer.upcast_ref(),
            form.sessions.upcast_ref(),
            form.login.upcast_ref(),
        ] {
            widget.set_sensitive(!busy);
        }
        if !busy {
            self.focus_field(form);
        }
    }

    /// Volta ao começo: a senha, sem pergunta do PAM na tela.
    fn reset_prompt(&self, form: &Form) {
        self.prompting.set(false);
        self.prompt_secret.set(true);
        form.prompt.set_visible(false);
        form.answer.set_visible(false);
        form.answer.set_text("");
        form.password.set_visible(true);
        form.password.set_text("");
    }

    /// Trocar de conta ou de sessão no meio de uma pergunta do PAM desiste daquele login.
    fn abandon(self: &Rc<Self>) {
        let Some(form) = self.form.borrow().clone() else { return };
        if !self.prompting.get() || self.busy.get() {
            return;
        }
        self.reset_prompt(&form);
        if let Some(mut client) = self.client.borrow_mut().take() {
            // Ocupado até o cancel voltar: um Enter no meio abriria outra conexão, e a volta deste
            // sobrescreveria a nova
            self.set_busy(&form, true);
            let app = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let client = gio::spawn_blocking(move || client.cancel().ok().map(|_| client)).await.ok().flatten();
                if let Some(app) = app.upgrade() {
                    *app.client.borrow_mut() = client;
                    let form = app.form.borrow().clone();
                    match form {
                        Some(form) => app.set_busy(&form, false),
                        None => app.busy.set(false),
                    }
                }
            });
        }
    }

    /// Enter ou o botão Entrar.
    fn submit(self: &Rc<Self>) {
        let Some(form) = self.form.borrow().clone() else { return };
        if self.busy.get() {
            return;
        }
        let Some(user) = self.chosen_user(&form) else {
            self.set_status(&form, &tr("Type a valid user name"), true);
            form.other.grab_focus();
            return;
        };
        let Some(session) = self.chosen_session(&form) else {
            self.set_status(&form, &tr("No session is installed"), true);
            return;
        };
        // O texto do campo vai para um Vec, zerado depois de enviado; o buffer do GtkPasswordEntry é o
        // seguro do GTK, limpo pelo set_text("") logo abaixo
        let take = |text: glib::GString| text.as_bytes().to_vec();
        let secret_field = !self.prompting.get() || self.prompt_secret.get();
        let typed = if secret_field { take(form.password.text()) } else { take(form.answer.text()) };
        form.password.set_text("");
        form.answer.set_text("");
        let prompting = self.prompting.get();
        if !prompting && typed.is_empty() {
            return;
        }
        self.set_busy(&form, true);
        self.set_status(&form, &tr("Checking…"), false);
        let client = self.client.borrow_mut().take();
        let app = Rc::downgrade(self);
        let session_id = session.id.clone();
        let user_name = user.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let mut typed = typed;
                let mut client = match client.map(Ok).unwrap_or_else(connect) {
                    Ok(client) => client,
                    Err(err) => {
                        zero(&mut typed);
                        return (None, Outcome::Broken(err.to_string()));
                    }
                };
                let outcome = if prompting {
                    let step = client.respond(std::str::from_utf8(&typed).unwrap_or(""));
                    zero(&mut typed);
                    match step {
                        Ok(step) => drive(&mut client, step, None, &session),
                        Err(err) => Outcome::Broken(err.to_string()),
                    }
                } else {
                    match client.create_session(&user) {
                        Ok(step) => drive(&mut client, step, Some(typed), &session),
                        Err(err) => {
                            zero(&mut typed);
                            Outcome::Broken(err.to_string())
                        }
                    }
                };
                let keep = !matches!(outcome, Outcome::Broken(_));
                (keep.then_some(client), outcome)
            })
            .await
            .unwrap_or((None, Outcome::Broken("the login thread failed".into())));
            if let Some(app) = app.upgrade() {
                *app.client.borrow_mut() = result.0;
                app.on_outcome(result.1, &user_name, &session_id);
            }
        });
    }

    fn on_outcome(self: &Rc<Self>, outcome: Outcome, user: &str, session: &str) {
        let Some(form) = self.form.borrow().clone() else { return };
        match outcome {
            Outcome::Started => {
                let mut state = self.state.borrow_mut();
                state.remember(user, session);
                if let Err(err) = state.save(&self.state_file) {
                    log(&format!("could not save {}: {err}", self.state_file.display()));
                }
                self.main_loop.quit();
            }
            Outcome::Prompt { secret, text, notes } => {
                self.prompting.set(true);
                self.prompt_secret.set(secret);
                form.prompt.set_text(text.trim());
                form.prompt.set_visible(true);
                form.password.set_visible(secret);
                form.answer.set_visible(!secret);
                self.set_busy(&form, false);
                self.set_status(&form, &notes.join("\n"), false);
            }
            Outcome::Failed { auth, text, notes } => {
                self.reset_prompt(&form);
                self.set_busy(&form, false);
                let mut message = if auth { tr("Wrong password") } else { format!("{} {text}", tr("Could not log in:")) };
                if let Some(note) = notes.last() {
                    message = format!("{note}\n{message}");
                }
                if !auth {
                    // Sem o nome digitado: no "Outro usuário", pode ser a senha por engano (a tela mostra o texto todo)
                    log(&format!("login failed: {}", users::redact_user(&text, user)));
                }
                self.set_status(&form, &message, true);
                shake(&form.password);
            }
            Outcome::Broken(err) => {
                log(&format!("greetd: {err}"));
                self.reset_prompt(&form);
                self.set_busy(&form, false);
                self.set_status(&form, &tr("The login service did not answer: try again"), true);
            }
        }
    }

    fn refresh_clock(&self) {
        let now = sl::local_now();
        let (time, date) = (sl::time_text(&now), sl::date_text(&now, &self.time_locale));
        for screen in self.screens.borrow().iter() {
            screen.time.set_text(&time);
            screen.date.set_text(&date);
        }
    }

    fn refresh_layout(self: &Rc<Self>) {
        let app = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let (name, many) = gio::spawn_blocking(read_layout).await.unwrap_or_default();
            let Some(app) = app.upgrade() else { return };
            let form = app.form.borrow().clone();
            if let Some(form) = form {
                form.layout.set_label(&name);
                form.layout.set_visible(!name.is_empty());
                form.layout.set_sensitive(many);
            }
        });
    }

    fn switch_layout(self: &Rc<Self>) {
        std::thread::spawn(|| output_within(Command::new("swaymsg").args(["input", "type:keyboard", "xkb_switch_layout", "next"]), SWAYMSG_TIMEOUT));
        let app = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(300), move || {
            if let Some(app) = app.upgrade() {
                app.refresh_layout();
            }
        });
    }

    /// Recria as telas: uma por monitor, com o formulário na primeira. O texto digitado se perde, a
    /// conversa com o greetd não.
    fn build_screens(self: &Rc<Self>) {
        for screen in self.screens.borrow_mut().drain(..) {
            screen.window.destroy();
        }
        self.abandon();
        *self.form.borrow_mut() = None;
        let monitors = self.display.monitors();
        let mut list: Vec<gdk::Monitor> = (0..monitors.n_items()).filter_map(|i| monitors.item(i).and_downcast::<gdk::Monitor>()).collect();
        // O formulário no monitor de cima à esquerda (o 0,0 do layout, normalmente o principal): a ordem da
        // lista do GDK é a de chegada, não a do layout
        list.sort_by_key(|m| {
            let g = m.geometry();
            (g.y(), g.x())
        });
        let mut screens = Vec::new();
        for monitor in &list {
            screens.push(self.screen(monitor, screens.is_empty()));
        }
        *self.screens.borrow_mut() = screens;
        self.refresh_clock();
        if let Some(form) = self.form.borrow().clone() {
            self.show_user(&form);
        }
        self.refresh_layout();
    }

    fn screen(self: &Rc<Self>, monitor: &gdk::Monitor, main: bool) -> Screen {
        let window = gtk4::Window::new();
        window.add_css_class("greeter");
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_monitor(monitor);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.set_exclusive_zone(-1);
        window.set_keyboard_mode(if main { KeyboardMode::Exclusive } else { KeyboardMode::None });
        window.set_namespace("swaydesk-greeter");

        let overlay = gtk4::Overlay::new();
        let center = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        center.set_halign(gtk4::Align::Center);
        center.set_valign(gtk4::Align::Center);
        let time = label("time");
        let date = label("date");
        center.append(&time);
        center.append(&date);
        if main {
            let form = self.form_widgets();
            center.append(&form.0);
            overlay.add_overlay(&form.1);
            overlay.add_overlay(&form.2);
        }
        overlay.set_child(Some(&center));
        window.set_child(Some(&overlay));
        window.present();
        Screen { window, time, date }
    }

    /// O cartão, o canto do teclado e os botões de energia; guarda o [`Form`].
    fn form_widgets(self: &Rc<Self>) -> (gtk4::Box, gtk4::Box, gtk4::Box) {
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        card.add_css_class("card");
        card.set_margin_top(36);
        card.set_size_request(340, -1);

        let avatar = gtk4::Overlay::new();
        avatar.add_css_class("avatar");
        avatar.set_overflow(gtk4::Overflow::Hidden);
        avatar.set_halign(gtk4::Align::Center);
        // Um Image de tamanho fixo: o Picture cresceria com a largura do cartão (altura pela largura)
        let photo = gtk4::Image::new();
        photo.set_pixel_size(AVATAR);
        avatar.set_child(Some(&photo));
        let letters = label("initials");
        avatar.add_overlay(&letters);
        card.append(&avatar);
        let name = label("name");
        name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        name.set_max_width_chars(NAME_CHARS);
        card.append(&name);

        // As contas pelo nome completo (o login quando o GECOS repete outro) e "Outro usuário…"
        let mut labels: Vec<String> = self.users.iter().map(|u| short(&sl::real_name(u, Path::new(sl::ACCOUNTS)))).collect();
        for i in 0..labels.len() {
            if labels.iter().filter(|l| **l == labels[i]).count() > 1 {
                labels[i] = short(&format!("{} ({})", labels[i], self.users[i].name));
            }
        }
        labels.push(tr("Other user…"));
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let users = gtk4::DropDown::from_strings(&label_refs);
        users.set_visible(!self.users.is_empty());
        card.append(&users);
        let other = gtk4::Entry::new();
        other.add_css_class("field");
        other.set_placeholder_text(Some(&tr("User name")));
        other.set_max_length(256);
        card.append(&other);

        let prompt = label("prompt");
        prompt.set_xalign(0.0);
        prompt.set_wrap(true);
        prompt.set_max_width_chars(NAME_CHARS);
        prompt.set_visible(false);
        card.append(&prompt);
        let password = gtk4::PasswordEntry::new();
        password.add_css_class("field");
        password.set_show_peek_icon(true);
        password.set_property("placeholder-text", tr("Password"));
        card.append(&password);
        let answer = gtk4::Entry::new();
        answer.add_css_class("field");
        answer.set_visible(false);
        card.append(&answer);

        let sessions_owned: Vec<String> = self.sessions.iter().map(|s| short(&s.name)).collect();
        let sessions_labels: Vec<&str> = sessions_owned.iter().map(String::as_str).collect();
        let sessions = gtk4::DropDown::from_strings(&sessions_labels);
        sessions.set_tooltip_text(Some(&tr("Session")));
        sessions.set_visible(self.sessions.len() > 1);
        card.append(&sessions);

        let status = label("status");
        status.set_wrap(true);
        status.set_max_width_chars(NAME_CHARS + 8);
        status.set_justify(gtk4::Justification::Center);
        card.append(&status);
        let login = gtk4::Button::with_label(&tr("Log in"));
        login.add_css_class("login");
        card.append(&login);

        let corner = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        corner.add_css_class("corner");
        corner.set_halign(gtk4::Align::End);
        corner.set_valign(gtk4::Align::Start);
        corner.set_margin_top(20);
        corner.set_margin_end(24);
        let layout = gtk4::Button::with_label("");
        layout.add_css_class("flat-btn");
        layout.set_focus_on_click(false);
        layout.set_tooltip_text(Some(&tr("Keyboard layout")));
        layout.set_visible(false);
        corner.append(&layout);

        let power = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        power.set_halign(gtk4::Align::End);
        power.set_valign(gtk4::Align::End);
        power.set_margin_end(24);
        power.set_margin_bottom(20);
        for (icon, tip, verb) in [(ICON_SUSPEND, tr("Suspend"), "suspend"), (ICON_REBOOT, tr("Restart"), "reboot"), (ICON_POWEROFF, tr("Shut down"), "poweroff")] {
            let button = flat_button(icon, &tip);
            button.connect_clicked(move |_| {
                std::thread::spawn(move || {
                    if output_within(Command::new("systemctl").arg(verb), Duration::from_secs(20)).is_none() {
                        log(&format!("systemctl {verb} failed"));
                    }
                });
            });
            power.append(&button);
        }

        let form = Rc::new(Form { users, other, photo, letters, name, prompt, password, answer, sessions, login, status, layout });
        let weak = Rc::downgrade(self);
        form.users.connect_selected_notify(move |_| {
            if let Some(app) = weak.upgrade() {
                app.abandon();
                if let Some(form) = app.form.borrow().clone() {
                    app.set_status(&form, "", false);
                    app.show_user(&form);
                }
            }
        });
        let weak = Rc::downgrade(self);
        form.sessions.connect_selected_notify(move |_| {
            if let Some(app) = weak.upgrade() {
                app.abandon();
            }
        });
        let weak = Rc::downgrade(self);
        form.other.connect_activate(move |_| {
            if let Some(app) = weak.upgrade() {
                if let Some(form) = app.form.borrow().clone() {
                    form.password.grab_focus();
                }
            }
        });
        for entry in [form.password.upcast_ref::<gtk4::Widget>(), form.answer.upcast_ref()] {
            let weak = Rc::downgrade(self);
            let activate = move || {
                if let Some(app) = weak.upgrade() {
                    app.submit();
                }
            };
            if let Some(entry) = entry.downcast_ref::<gtk4::PasswordEntry>() {
                entry.connect_activate(move |_| activate());
            } else if let Some(entry) = entry.downcast_ref::<gtk4::Entry>() {
                entry.connect_activate(move |_| activate());
            }
        }
        let weak = Rc::downgrade(self);
        form.login.connect_clicked(move |_| {
            if let Some(app) = weak.upgrade() {
                app.submit();
            }
        });
        let weak = Rc::downgrade(self);
        form.layout.connect_clicked(move |_| {
            if let Some(app) = weak.upgrade() {
                app.switch_layout();
            }
        });
        // A conta inicial: a última que entrou, se ainda existir; senão a primeira
        let last = self.state.borrow().last_user.clone();
        if let Some(index) = self.users.iter().position(|u| u.name == last) {
            form.users.set_selected(index as u32);
        } else if self.users.is_empty() && users::valid_username(&last) {
            form.other.set_text(&last);
        }
        *self.form.borrow_mut() = Some(form);
        (card, corner, power)
    }
}

/// O tremor do campo: as margens do bloqueio, em 400 ms.
fn shake(widget: &impl IsA<gtk4::Widget>) {
    const FRAMES: [(f64, f64); 6] = [(0.0, 0.0), (16.0, 0.0), (0.0, 16.0), (12.0, 0.0), (0.0, 8.0), (0.0, 0.0)];
    let widget = widget.clone().upcast::<gtk4::Widget>();
    let start = Instant::now();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        let t = (start.elapsed().as_secs_f64() * 1000.0 / SHAKE_MS).min(1.0);
        let position = t * 5.0;
        let i = (position.floor() as usize).min(4);
        let f = position - i as f64;
        let (a, b) = (FRAMES[i], FRAMES[i + 1]);
        widget.set_margin_start((a.0 + (b.0 - a.0) * f).round() as i32);
        widget.set_margin_end((a.1 + (b.1 - a.1) * f).round() as i32);
        if t >= 1.0 { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
    });
}

fn env_path(name: &str, fallback: &str) -> PathBuf {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(fallback))
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-greeter\n",
    "    The login screen, started by greetd through /usr/share/swaydesk-greeter/sway.conf.",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().len() > 1 {
        eprintln!("usage: swaydesk-greeter (started by greetd, through /usr/share/swaydesk-greeter/sway.conf)");
        return ExitCode::from(2);
    }
    // As teclas mortas (´ + e → é) pelo método de entrada do próprio GTK. Com o "wayland", que o GTK 4.22
    // (Ubuntu 26.04) escolhe no locale C.UTF-8 do Ubuntu Server, elas iriam ao input method do Sway, que a
    // tela de login não tem, e o acento se perdia: quem tem acento na senha não entrava. Antes de qualquer
    // linha nova (o set_var não é seguro com outras linhas lendo o ambiente)
    if std::env::var_os("GTK_IM_MODULE").is_none_or(|v| v.is_empty()) {
        std::env::set_var("GTK_IM_MODULE", "simple");
    }
    // O ç no US internacional: o "simple" usa a tabela do locale, que no en_US dá ć para ' + c. O GTK só lê
    // uma tabela própria do $XDG_CONFIG_HOME/gtk-4.0/Compose ou do ~/.XCompose, e o usuário da tela não tem
    // um HOME nosso: a pasta do pacote, só com essa tabela (a da sessão das pessoas, a mesma do ~/.XCompose)
    let intl = keyboard::system_keyboard(Path::new("/etc/default/keyboard")).is_some_and(|(_, text)| keyboard::is_us_intl(&text));
    if intl && Path::new(INTL_CONFIG_HOME).join("gtk-4.0/Compose").is_file() {
        std::env::set_var("XDG_CONFIG_HOME", INTL_CONFIG_HOME);
    }
    // O teclado e o cursor já, enquanto o GTK abre
    std::thread::spawn(|| {
        apply_system_keyboard();
        apply_cursor();
    });
    swaydesk_common::renderer::use_cairo_renderer();
    if let Err(err) = gtk4::init() {
        log(&format!("could not open the display: {err}"));
        return ExitCode::FAILURE;
    }
    let lang = system::system_lang(&system::LOCALE_FILES.map(Path::new));
    let locale_env = system::system_locale_env(&system::LOCALE_FILES.map(Path::new));
    let (config, time_locales) = system::greeter_setup(&env_path("SWAYDESK_GREETER_CONFIG", system::CONFIG_FILE), &locale_env);
    swaydesk_common::i18n::init(&config);
    // A data no LC_TIME do sistema e, sem ele gerado (o Arch com o locale.conf mudado à mão, sem locale-gen),
    // na língua dos textos; sem nenhum, no formato do C, sem quebrar
    let (time_locale, missing) = sl::set_time_locale(&time_locales);
    if let Some(line) = sl::missing_time_locale_line(&missing, &time_locale) {
        log(&line);
    }
    let Some(display) = gdk::Display::default() else {
        log("no display");
        return ExitCode::FAILURE;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&format!("{}{}{CSS}", config::font_css(None), config::css_colors(&config::palette(&config))));
    #[allow(deprecated)] // sem substituto no GTK 4.18
    gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(true);
    }

    let data_dirs = std::env::var("XDG_DATA_DIRS").ok();
    let path = std::env::var("PATH").unwrap_or_default();
    let main_loop = glib::MainLoop::new(None, false);
    let state_file = env_path("SWAYDESK_GREETER_STATE", STATE_FILE);
    let app = Rc::new(App {
        main_loop: main_loop.clone(),
        display: display.clone(),
        users: users::users(Path::new("/etc/passwd"), Path::new("/etc/login.defs")),
        sessions: sessions::sessions(&sessions::session_dirs(data_dirs.as_deref()), &lang, &path),
        state: RefCell::new(State::load(&state_file)),
        state_file,
        time_locale,
        screens: RefCell::new(Vec::new()),
        form: RefCell::new(None),
        client: RefCell::new(None),
        busy: Cell::new(false),
        photo_generation: Cell::new(0),
        prompting: Cell::new(false),
        prompt_secret: Cell::new(true),
    });
    if app.sessions.is_empty() {
        log("no session in wayland-sessions");
    }
    app.build_screens();
    let weak = Rc::downgrade(&app);
    display.monitors().connect_items_changed(move |_, _, _, _| {
        if let Some(app) = weak.upgrade() {
            // Os monitores chegam um a um: espera o GDK assentar antes de recriar
            let weak = Rc::downgrade(&app);
            glib::idle_add_local_once(move || {
                if let Some(app) = weak.upgrade() {
                    app.build_screens();
                }
            });
        }
    });
    if let Some(keyboard) = display.default_seat().and_then(|seat| seat.keyboard()) {
        let weak = Rc::downgrade(&app);
        keyboard.connect_caps_lock_state_notify(move |_| {
            if let Some(app) = weak.upgrade() {
                app.show_caps();
            }
        });
    }
    let weak = Rc::downgrade(&app);
    glib::timeout_add_seconds_local(1, move || match weak.upgrade() {
        Some(app) => {
            app.refresh_clock();
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
    let weak = Rc::downgrade(&app);
    glib::timeout_add_seconds_local(LAYOUT_EVERY_S, move || match weak.upgrade() {
        Some(app) => {
            app.refresh_layout();
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
    main_loop.run();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os filhos (swaymsg, systemctl) nascem sem sinais bloqueados, mesmo abertos de uma linha do GIO.
    #[test]
    fn children_start_with_no_blocked_signals() {
        let blocked = std::thread::spawn(|| unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigfillset(&mut set);
            libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
            output_within(Command::new("grep").args(["SigBlk", "/proc/self/status"]), Duration::from_secs(5))
        })
        .join()
        .unwrap()
        .expect("grep");
        let text = String::from_utf8_lossy(&blocked);
        assert!(text.trim_end().ends_with("0000000000000000"), "{text}");
    }

    #[test]
    fn photos_only_small_png_or_jpeg_regular_files() {
        let dir = std::env::temp_dir().join(format!("swaydesk-greeter-photo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("ana");
        gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 300, 200).unwrap().savev(&png, "png", &[]).unwrap();
        let (_, width, height, _, alpha) = decode_photo(&png).unwrap();
        assert_eq!((width, height, alpha), (96, 96, true));
        // Um link, um arquivo de outro formato, um vazio, um grande demais e um FIFO: nada
        let link = dir.join("link");
        std::os::unix::fs::symlink(&png, &link).unwrap();
        assert!(decode_photo(&link).is_none());
        std::fs::write(dir.join("gif"), b"GIF89a....").unwrap();
        assert!(decode_photo(&dir.join("gif")).is_none());
        std::fs::write(dir.join("empty"), b"").unwrap();
        assert!(decode_photo(&dir.join("empty")).is_none());
        let huge = dir.join("huge");
        gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, false, 8, 3000, 10).unwrap().savev(&huge, "png", &[]).unwrap();
        assert!(decode_photo(&huge).is_none());
        let fifo = std::ffi::CString::new(dir.join("fifo").to_str().unwrap()).unwrap();
        unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) };
        assert!(decode_photo(&dir.join("fifo")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_size_comes_from_the_header() {
        // Um PNG que declara 20000x20000: recusado sem decodificar (só o cabeçalho existe)
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&20000u32.to_be_bytes());
        png.extend_from_slice(&20000u32.to_be_bytes());
        assert_eq!(image_size(&png), Some((20000, 20000)));
        let dir = std::env::temp_dir().join(format!("swaydesk-greeter-header-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bomb"), [png.as_slice(), &[0u8; 64]].concat()).unwrap();
        assert!(decode_photo(&dir.join("bomb")).is_none());
        // Um JPEG de verdade: o SOF0 depois do APP0
        let jpeg = dir.join("photo.jpg");
        gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, false, 8, 120, 80).unwrap().savev(&jpeg, "jpeg", &[]).unwrap();
        assert_eq!(image_size(&std::fs::read(&jpeg).unwrap()), Some((120, 80)));
        // Menor que o avatar: sem ampliar, só o quadrado do meio (o Image ajusta ao tamanho)
        assert_eq!(decode_photo(&jpeg).map(|p| (p.1, p.2)), Some((80, 80)));
        assert_eq!(image_size(b"\xff\xd8\xff\xe0\x00"), None);
        assert_eq!(image_size(b"GIF89a"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_names_are_cut() {
        assert_eq!(short("Ana"), "Ana");
        let long = "x".repeat(100);
        assert_eq!(short(&long).chars().count(), NAME_CHARS as usize);
        assert!(short(&long).ends_with('…'));
    }

    #[test]
    fn the_password_is_zeroed() {
        let mut secret = b"s3nha".to_vec();
        zero(&mut secret);
        assert_eq!(secret, [0; 5]);
    }
}
