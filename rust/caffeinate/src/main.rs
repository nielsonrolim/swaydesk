//! A cafeína fora da sessão do Swaydesk (um Sway puro, com o swayidle do próprio usuário): em vez de trocar o
//! swayidle, como o sway-idle faz na sessão, segura um inibidor de inatividade (zwp_idle_inhibit_manager_v1),
//! que o swayidle respeita (ext-idle-notify). O sway-caffeine o abre ao ligar e o encerra ao desligar.
//!
//! Uma superfície de 1×1 transparente na camada de fundo (layer-shell) em cada monitor, sem área de entrada.
//! O Sway 1.12 só conta o inibidor de uma layer surface já mapeada, numa saída ligada, e confere isso quando o
//! inibidor é criado: aqui ele vem depois do mapeamento (um sync depois do commit com o buffer). Um monitor
//! desativado ou desconectado não conta (no Sway 1.12, nem um com a energia desligada, `output X power off`);
//! um que volta (ou um novo) ganha a sua superfície. Com todos assim, a inatividade pode disparar.
//!
//! Roda como `sway-caffeinate` (o nome do papel, que o sway-caffeine confere no /proc), um só por vez (a
//! trava), com o arquivo de PID em $XDG_RUNTIME_DIR. Sai com o fim da sessão (a conexão cai) ou com um sinal,
//! e apaga o arquivo de PID.
use std::collections::HashMap;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::process::ExitCode;
use std::time::Duration;

use swaydesk_common::resident;
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_output, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::wp::idle_inhibit::zv1::client::{zwp_idle_inhibit_manager_v1, zwp_idle_inhibitor_v1};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

const PROGRAM: &str = "sway-caffeinate";

/// A superfície de um monitor.
struct Spot {
    surface: wl_surface::WlSurface,
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    mapped: bool,
    inhibitor: Option<zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1>,
}

impl Spot {
    fn destroy(self) {
        if let Some(inhibitor) = self.inhibitor {
            inhibitor.destroy();
        }
        self.layer.destroy();
        self.surface.destroy();
    }
}

#[derive(Default)]
struct State {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    inhibit: Option<zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1>,
    buffer: Option<wl_buffer::WlBuffer>,
    /// Os monitores pelo nome do global, cada um com a sua superfície (se o compositor não a fechou)
    outputs: HashMap<u32, (wl_output::WlOutput, Option<Spot>)>,
    /// Monitores anunciados antes de haver o layer-shell e o buffer: ganham a superfície depois
    ready: bool,
}

impl State {
    fn add_spot(&mut self, name: u32, qh: &QueueHandle<Self>) {
        let (Some(compositor), Some(layer_shell)) = (&self.compositor, &self.layer_shell) else { return };
        let Some((output, spot)) = self.outputs.get_mut(&name) else { return };
        if spot.is_some() {
            return;
        }
        let surface = compositor.create_surface(qh, ());
        let region = compositor.create_region(qh, ()); // vazia: os cliques passam
        surface.set_input_region(Some(&region));
        region.destroy();
        let layer = layer_shell.get_layer_surface(&surface, Some(output), zwlr_layer_shell_v1::Layer::Background,
                                                  PROGRAM.into(), qh, name);
        layer.set_size(1, 1);
        layer.set_anchor(zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Left);
        layer.set_exclusive_zone(-1); // não empurra a barra nem as janelas
        layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        surface.commit();
        *spot = Some(Spot { surface, layer, mapped: false, inhibitor: None });
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(state: &mut Self, registry: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection,
             qh: &QueueHandle<Self>) {
        match event {
            wl_registry::Event::Global { name, interface, version } => match interface.as_str() {
                "wl_compositor" => state.compositor = Some(registry.bind(name, version.min(4), qh, ())),
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "zwlr_layer_shell_v1" => state.layer_shell = Some(registry.bind(name, version.min(4), qh, ())),
                "zwp_idle_inhibit_manager_v1" => state.inhibit = Some(registry.bind(name, 1, qh, ())),
                "wl_output" => {
                    let output = registry.bind(name, version.min(4), qh, ());
                    state.outputs.insert(name, (output, None));
                    if state.ready {
                        state.add_spot(name, qh); // um monitor ligado ou conectado agora
                    }
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                // Um monitor desativado ou desconectado: a superfície dele já não conta
                if let Some((output, spot)) = state.outputs.remove(&name) {
                    if let Some(spot) = spot {
                        spot.destroy();
                    }
                    if output.version() >= 3 {
                        output.release();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, u32> for State {
    fn event(state: &mut Self, layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event,
             name: &u32, conn: &Connection, qh: &QueueHandle<Self>) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, .. } => {
                layer.ack_configure(serial);
                let buffer = state.buffer.clone();
                let Some((_, Some(spot))) = state.outputs.get_mut(name) else { return };
                if !spot.mapped {
                    spot.surface.attach(buffer.as_ref(), 0, 0);
                    spot.surface.damage(0, 0, 1, 1);
                    spot.surface.commit();
                    spot.mapped = true;
                    // O inibidor só quando o Sway já tiver mapeado a superfície (a resposta deste sync)
                    conn.display().sync(qh, *name);
                } else {
                    spot.surface.commit();
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                // O compositor tirou a superfície (o monitor sumiu): volta com o monitor
                if let Some((_, spot)) = state.outputs.get_mut(name) {
                    if let Some(spot) = spot.take() {
                        spot.destroy();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, u32> for State {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, event: wl_callback::Event, name: &u32, _: &Connection,
             qh: &QueueHandle<Self>) {
        if let wl_callback::Event::Done { .. } = event {
            let Some(inhibit) = state.inhibit.clone() else { return };
            if let Some((_, Some(spot))) = state.outputs.get_mut(name) {
                if spot.inhibitor.is_none() {
                    spot.inhibitor = Some(inhibit.create_inhibitor(&spot.surface, qh, ()));
                    spot.surface.commit();
                }
            }
        }
    }
}

macro_rules! ignore_events {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for State {
            fn event(_: &mut Self, _: &$t, _: <$t as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore_events!(wl_compositor::WlCompositor, wl_shm::WlShm, wl_shm_pool::WlShmPool, wl_buffer::WlBuffer,
               wl_surface::WlSurface, wl_region::WlRegion, wl_output::WlOutput, zwlr_layer_shell_v1::ZwlrLayerShellV1,
               zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1, zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1);

/// Um pixel ARGB8888 transparente, num memfd.
fn pixel() -> std::io::Result<OwnedFd> {
    // SAFETY: memfd_create com um nome C válido; o fd devolvido passa a ser nosso
    let fd = unsafe { libc::memfd_create(c"sway-caffeinate".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: fd válido, aberto acima, e de mais ninguém
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    std::fs::File::from(fd.try_clone()?).set_len(4)?;
    Ok(fd)
}

/// SIGTERM, SIGINT e SIGHUP chegam a uma linha própria, que apaga o arquivo de PID e sai. Bloqueados antes de
/// qualquer outra linha: todas herdam a máscara.
fn exit_on_signals(pidfile: std::path::PathBuf) {
    // SAFETY: sigset_t inicializado pelo sigemptyset antes do uso; só chamadas de máscara e espera de sinais
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::sigaddset(&mut set, sig);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        std::thread::spawn(move || {
            let mut sig = 0;
            libc::sigwait(&set, &mut sig);
            resident::remove_own_pid(&pidfile, PROGRAM);
            std::process::exit(0);
        });
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{PROGRAM}: {message}");
    ExitCode::FAILURE
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-caffeinate\n",
    "    Holds a Wayland idle inhibitor on every monitor until it is stopped (caffeine outside the Swaydesk session).",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o sway-caffeine confere que o binário carrega
    }
    resident::run_as(PROGRAM);
    resident::set_process_name(PROGRAM);
    // Um por vez: ligar de novo, ou dois pedidos juntos, não abrem outro
    let Some(_lock) = resident::take_lock(&resident::lockfile(PROGRAM), Duration::from_secs(1)) else {
        return ExitCode::SUCCESS;
    };
    let pidfile = resident::pidfile(PROGRAM);
    exit_on_signals(pidfile.clone());
    let code = run();
    resident::remove_own_pid(&pidfile, PROGRAM);
    code
}

fn run() -> ExitCode {
    let conn = match Connection::connect_to_env() {
        Ok(conn) => conn,
        Err(err) => return fail(&format!("no Wayland display: {err}")),
    };
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = State::default();
    if let Err(err) = queue.roundtrip(&mut state) {
        return fail(&format!("Wayland: {err}"));
    }
    let (Some(shm), true, true, true) =
        (state.shm.clone(), state.compositor.is_some(), state.layer_shell.is_some(), state.inhibit.is_some())
    else {
        return fail("the compositor lacks wl_shm, zwlr_layer_shell_v1 or zwp_idle_inhibit_manager_v1");
    };
    let fd = match pixel() {
        Ok(fd) => fd,
        Err(err) => return fail(&format!("no shared memory for the surface: {err}")),
    };
    let pool = shm.create_pool(fd.as_fd(), 4, &qh, ());
    state.buffer = Some(pool.create_buffer(0, 1, 1, 4, wl_shm::Format::Argb8888, &qh, ()));
    state.ready = true;
    let names: Vec<u32> = state.outputs.keys().copied().collect();
    for name in names {
        state.add_spot(name, &qh);
    }
    // O arquivo de PID depois de conectar: quem o vê sabe que a cafeína já vale (ou está prestes a valer)
    if let Err(err) = resident::write_pid(&resident::pidfile(PROGRAM)) {
        eprintln!("{PROGRAM}: could not write the pidfile: {err}");
    }
    // Até a sessão acabar: a conexão cai e o laço termina
    while queue.blocking_dispatch(&mut state).is_ok() {}
    ExitCode::SUCCESS
}
