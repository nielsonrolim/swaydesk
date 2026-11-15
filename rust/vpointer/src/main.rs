//! Ponteiro virtual para os testes de interface, no Sway headless: sem dispositivo de entrada, o seat
//! não tem ponteiro, e o `swaymsg seat … cursor press` não chega a nenhum cliente. Este programa cria
//! um ponteiro pelo `zwlr_virtual_pointer_v1` (o mesmo do `wlrctl`) e manda os movimentos e cliques.
//!
//! uso: swaydesk-vpointer [--size LxA] AÇÃO...
//!   move X Y          leva o ponteiro a (X, Y), em pixels do layout (a tela de LxA, padrão 1280x720).
//!                     Com uma saída em 0,0; com mais de uma, --size é o do layout inteiro
//!   click [left|right|middle]
//!   scroll N          N passos da roda do mouse (positivo desce a lista), um por vez
//!   sleep MS
//! Exemplo: swaydesk-vpointer --size 1280x720 move 100 600 click
//! Antes da primeira ação, espera os clientes verem o ponteiro novo (o seat ganha a capacidade).
use std::time::{Duration, Instant};

use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1};

#[derive(Default)]
struct State {
    seat: Option<wl_seat::WlSeat>,
    manager: Option<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(state: &mut Self, registry: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection,
             qh: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => state.seat = Some(registry.bind(name, version.min(7), qh, ())),
                "zwlr_virtual_pointer_manager_v1" => state.manager = Some(registry.bind(name, version.min(2), qh, ())),
                _ => {}
            }
        }
    }
}

macro_rules! ignore_events {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for State {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore_events!(wl_seat::WlSeat, zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
               zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1);

/// Os códigos do Linux (input-event-codes.h).
fn button_code(name: &str) -> Option<u32> {
    match name {
        "left" => Some(0x110),
        "right" => Some(0x111),
        "middle" => Some(0x112),
        _ => None,
    }
}

fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.split_once('x')?;
    let size = (w.parse().ok()?, h.parse().ok()?);
    (size.0 > 0 && size.1 > 0).then_some(size) // 0 estouraria o size - 1 do move
}

fn usage() -> ! {
    eprintln!("usage: swaydesk-vpointer [--size WxH] (move X Y | click|press|release [left|right|middle] | scroll N | sleep MS)...");
    std::process::exit(2);
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut size = (1280, 720);
    if args.first().map(String::as_str) == Some("--size") {
        size = args.get(1).and_then(|s| parse_size(s)).unwrap_or_else(|| usage());
        args.drain(..2);
    }
    if args.is_empty() {
        usage();
    }
    let conn = Connection::connect_to_env().unwrap_or_else(|err| {
        eprintln!("swaydesk-vpointer: no Wayland display: {err}");
        std::process::exit(1);
    });
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = State::default();
    queue.roundtrip(&mut state).expect("roundtrip");
    let Some(manager) = state.manager.clone() else {
        eprintln!("swaydesk-vpointer: the compositor has no zwlr_virtual_pointer_manager_v1");
        std::process::exit(1);
    };
    let pointer = manager.create_virtual_pointer(state.seat.as_ref(), &qh, ());
    queue.roundtrip(&mut state).expect("roundtrip");
    // O seat ganhou a capacidade de ponteiro: os clientes precisam de um instante para pegar o wl_pointer
    std::thread::sleep(Duration::from_millis(300));

    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u32;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "move" => {
                let (Some(x), Some(y)) = (args.get(i + 1).and_then(|v| v.parse::<u32>().ok()),
                                          args.get(i + 2).and_then(|v| v.parse::<u32>().ok())) else { usage() };
                pointer.motion_absolute(now(), x.min(size.0 - 1), y.min(size.1 - 1), size.0, size.1);
                pointer.frame();
                i += 3;
            }
            "click" => {
                let (code, used) = match args.get(i + 1).and_then(|b| button_code(b)) {
                    Some(code) => (code, 2),
                    None => (button_code("left").unwrap(), 1),
                };
                pointer.button(now(), code, wl_pointer::ButtonState::Pressed);
                pointer.frame();
                queue.roundtrip(&mut state).expect("roundtrip");
                std::thread::sleep(Duration::from_millis(30));
                pointer.button(now(), code, wl_pointer::ButtonState::Released);
                pointer.frame();
                i += used;
            }
            // O botão segurado (press) ou solto (release) separados, para testar arrastos
            "press" | "release" => {
                let (code, used) = match args.get(i + 1).and_then(|b| button_code(b)) {
                    Some(code) => (code, 2),
                    None => (button_code("left").unwrap(), 1),
                };
                let state = if args[i] == "press" { wl_pointer::ButtonState::Pressed } else { wl_pointer::ButtonState::Released };
                pointer.button(now(), code, state);
                pointer.frame();
                i += used;
            }
            "scroll" => {
                let Some(steps) = args.get(i + 1).and_then(|v| v.parse::<i32>().ok()) else { usage() };
                for _ in 0..steps.unsigned_abs() {
                    pointer.axis_source(wl_pointer::AxisSource::Wheel);
                    pointer.axis_discrete(now(), wl_pointer::Axis::VerticalScroll, 15.0 * f64::from(steps.signum()), steps.signum());
                    pointer.frame();
                    queue.roundtrip(&mut state).expect("roundtrip");
                    std::thread::sleep(Duration::from_millis(30));
                }
                i += 2;
            }
            "sleep" => {
                let Some(ms) = args.get(i + 1).and_then(|v| v.parse::<u64>().ok()) else { usage() };
                queue.roundtrip(&mut state).expect("roundtrip");
                std::thread::sleep(Duration::from_millis(ms));
                i += 2;
            }
            _ => usage(),
        }
        queue.roundtrip(&mut state).expect("roundtrip");
    }
    pointer.destroy();
    queue.roundtrip(&mut state).ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_buttons() {
        assert_eq!(parse_size("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_size("x"), None);
        assert_eq!(parse_size("0x720"), None);
        assert_eq!(button_code("right"), Some(0x111));
        assert_eq!(button_code("other"), None);
    }
}
