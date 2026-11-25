//! swaydesk-click-catcher --pid PID: o clique fora do lançador (o rofi do sway-launcher) o fecha, como o Esc.
//!
//! O rofi 1.7 (Debian 13) roda pelo Xwayland, e o clique numa janela Wayland nunca chega ao X; o 2.0 (Ubuntu
//! 26.04) desenha uma camada do tamanho do menu e não vê o clique fora dela. Daí estas camadas: uma por monitor,
//! na camada de cima do layer-shell, transparentes e sem teclado (as dos popups, swaydesk_ui::catcher_windows).
//! O Sway põe as janelas override-redirect do Xwayland (o rofi 1.7) e a camada overlay (o rofi 2.x) acima da
//! de cima: o clique no rofi continua sendo dele, e o teclado também. O 2.1 (Arch) cobre a tela com a própria
//! camada (overlay, acima desta) e trata ali o clique fora, mas ela não cobre a Waybar: esta cobre.
//!
//! A camada cobre a barra também (o clique na Waybar fecha o lançador). E o Sway só passa o ponteiro a uma
//! superfície nova no próximo movimento: sem mexer o mouse, o primeiro clique iria ao que estava embaixo. Daí,
//! logo depois de mapear, o `seat - cursor move` do Sway, que reavalia a superfície sob o ponteiro (cursor_rebase).
//! Um pixel para o lado e de volta, não zero: o rebase manda o enter e um motion na mesma posição, que o wlroots
//! descarta; o rofi 2.1 ignora a posição do enter e, sem um motion, toma qualquer clique por um clique fora do
//! menu. O segundo passo, na mesma superfície e noutra posição, manda o motion. O ponteiro termina onde estava.
//!
//! O processo é seguido por um pidfd: um clique o encerra (SIGTERM, como o pkill do sway-launcher) e o fim dele
//! encerra este. Com o pidfd, um número de processo reaproveitado nunca recebe o sinal.
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::ExitCode;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, glib};

/// O pidfd do processo, ou None se ele já terminou (ou o kernel não tem pidfd, antes do 5.3).
fn pidfd_open(pid: libc::pid_t) -> Option<OwnedFd> {
    // SAFETY: a syscall só lê os dois inteiros; um fd válido devolvido passa a ser deste processo
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

fn pidfd_signal(fd: &OwnedFd, signal: libc::c_int) {
    // SAFETY: o sinal vai ao processo do pidfd, sem info (NULL) e sem flags
    unsafe {
        libc::syscall(libc::SYS_pidfd_send_signal, fd.as_raw_fd(), signal, std::ptr::null::<libc::siginfo_t>(), 0);
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: swaydesk-click-catcher --pid PID");
    ExitCode::from(2)
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-click-catcher --pid PID\n",
    "    A click outside the launcher closes it, as Esc does; exits when PID exits.",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pid = match args.as_slice() {
        [flag, pid] if flag == "--pid" => match pid.parse::<libc::pid_t>() {
            Ok(pid) if pid > 0 => pid,
            _ => return usage(),
        },
        _ => return usage(),
    };
    let Some(pidfd) = pidfd_open(pid) else { return ExitCode::SUCCESS }; // já terminou: nada a pegar
    if gtk4::init().is_err() {
        return ExitCode::from(1);
    }
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(swaydesk_ui::BASE_CSS);
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    let pidfd = Rc::new(pidfd);
    let main_loop = glib::MainLoop::new(None, false);
    // O clique: encerra o processo; o fim dele (abaixo) encerra este
    let on_click: Rc<dyn Fn()> = Rc::new({
        let pidfd = pidfd.clone();
        move || pidfd_signal(&pidfd, libc::SIGTERM)
    });
    let windows = swaydesk_ui::catcher_windows("popup-click-catcher", true, on_click);
    // O ponteiro passa às camadas novas sem esperar um movimento: um instante depois do mapa, quando o Sway já
    // as pôs na tela
    // Depois do mapa de CADA camada (a da outra tela pode vir depois da primeira), algumas vezes nos primeiros
    // segundos: o Sway as põe na tela um pouco depois do mapa do GTK, e o rofi 2.1 mapeia a própria camada (a tela
    // inteira) às vezes depois destas; reavaliar quando ela já está lá passa o ponteiro parado sobre o menu a ela.
    // Reavaliar de novo não mexe o ponteiro
    let rebase = || {
        for ms in [120, 400, 800, 1500, 2500] {
            glib::timeout_add_local_once(std::time::Duration::from_millis(ms), || {
                let _ = swaydesk_ui::cmd::spawn_detached("swaymsg", &["seat - cursor move 1 0; seat - cursor move -1 0"]);
            });
        }
    };
    for window in &windows {
        // O GTK pode já tê-la mapeado no set_visible: aí, direto; senão, no mapa
        if window.is_mapped() {
            rebase();
        } else {
            window.connect_map(move |_| rebase());
        }
    }
    // O pidfd fica legível quando o processo termina
    glib::unix_fd_add_local(pidfd.as_raw_fd(), glib::IOCondition::IN | glib::IOCondition::HUP, {
        let main_loop = main_loop.clone();
        move |_, _| {
            main_loop.quit();
            glib::ControlFlow::Break
        }
    });
    main_loop.run();
    for window in windows {
        window.destroy();
    }
    ExitCode::SUCCESS
}
