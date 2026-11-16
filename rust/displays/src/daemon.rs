//! O serviço de perfis (`sway-displays daemon`, no laço do config do Sway) e o `sway-displays apply` (a
//! tampa do notebook, pelo bindswitch do config do Sway). O serviço: quando o conjunto de monitores
//! muda (plugar, desplugar) ou o config é relido, aplica o perfil guardado para o conjunto, como o kanshi.
//! Sem perfil, fica o que o Sway escolheu. Uma mudança feita à mão (swaymsg, o bindswitch da tampa) com o
//! mesmo conjunto não é desfeita: só a troca de conjunto e o reload contam.
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use swaydesk_common::resident;
use swaydesk_displays_logic::config::commands;
use swaydesk_displays_logic::outputs;
use swaydesk_displays_logic::plan::{self, Plan};
use swaydesk_displays_logic::profiles::{self, Profiles};

use crate::sway::{self, Event};
use crate::{preview_active, warn, PROGRAM};

/// Espera depois do último evento: plugar um monitor gera vários seguidos
const SETTLE: Duration = Duration::from_millis(500);

/// O conjunto atual, ordenado
fn current_set(list: &[outputs::Output]) -> Vec<String> {
    profiles::set_of(&outputs::keys(list))
}

/// Aplica o perfil do conjunto, se houver e se for preciso, e depois confere que alguma tela ficou ligada.
/// Devolve o conjunto visto (None: não leu).
pub fn apply_profile(why: &str) -> Option<Vec<String>> {
    let list = sway::get_outputs()?;
    let set = current_set(&list);
    let profiles = Profiles::load(&profiles::path()).unwrap_or_else(|err| {
        warn(&format!("ignoring the profiles: {err}"));
        Profiles::default()
    });
    match plan::plan(&list, &profiles, plan::lid_closed(plan::LID_ROOT.as_ref())) {
        Plan::NoProfile | Plan::Already => {}
        Plan::Refused => warn("the profile would turn every screen off: not applied"),
        Plan::Apply(configs) => match sway::run_checked(
            &commands(&configs),
            &configs.iter().map(|(name, config)| (name.clone(), config.enabled)).collect::<Vec<_>>(),
        ) {
            Ok(()) => warn(&format!("applied the profile for {} screen(s) ({why})", configs.len())),
            Err(err) => warn(&format!("could not apply the profile: {err}")),
        },
    }
    // A rede de segurança: nenhuma tela ligada (um perfil ou um config de outro conjunto) liga todas. O
    // Aplicar do app tem a volta em 15 s; isto cobre o que o Sway e o serviço fazem sozinhos
    rescue_screens(why);
    Some(set)
}

/// Se nenhuma tela está ligada, liga todas e confere no Sway que ligou. O Sway responde "ok" mesmo quando o
/// backend recusa (o "Backend commit failed" do sway.log, visto numa VM logo depois de um disable): tenta
/// de novo depois de uma pausa e, por fim, com o modo preferido de cada saída.
pub fn rescue_screens(why: &str) {
    const PAUSES: [u64; 3] = [0, 1000, 2000];
    for (attempt, pause) in PAUSES.iter().enumerate() {
        std::thread::sleep(Duration::from_millis(*pause));
        let Some(now) = sway::get_outputs() else { return };
        let command = if attempt + 1 < PAUSES.len() { plan::rescue(&now) } else { plan::rescue_with_mode(&now) };
        let Some(command) = command else {
            if attempt > 0 {
                warn(&format!("no screen was on: turned every screen on ({why})"));
            }
            return;
        };
        if let Err(err) = sway::run(&command) {
            warn(&format!("no screen is on, and turning them on failed: {err}"));
        }
    }
    std::thread::sleep(Duration::from_millis(500));
    match sway::get_outputs() {
        Some(now) if now.iter().any(|o| o.active) => warn(&format!("no screen was on: turned every screen on ({why})")),
        _ => warn(&format!("no screen is on, and Sway did not turn any on ({why})")),
    }
}

/// `apply`: aplica o perfil do conjunto agora (a tampa abriu ou fechou). Fora da prévia do app.
pub fn apply_now() -> ExitCode {
    if !preview_active() {
        apply_profile("lid");
    }
    ExitCode::SUCCESS
}

pub fn run() -> ExitCode {
    resident::set_process_name(PROGRAM);
    // Um só serviço por sessão: o segundo sai em silêncio
    let Some(_lock) = resident::take_lock(&resident::lockfile("sway-displays-daemon"), Duration::ZERO) else {
        return ExitCode::SUCCESS;
    };
    let (mut child, mut reader) = match sway::subscribe() {
        Ok(pair) => pair,
        Err(err) => {
            warn(&format!("swaymsg: {err}"));
            return ExitCode::FAILURE;
        }
    };
    let (tx, rx) = mpsc::channel::<Event>();
    std::thread::spawn(move || {
        while let Some(line) = sway::next_line(&mut reader) {
            if let Some(event) = sway::event(&line) {
                if tx.send(event).is_err() {
                    break;
                }
            }
        }
    });
    // Assinado antes de ler: um monitor plugado no meio não se perde
    let mut last = if preview_active() { sway::get_outputs().map(|list| current_set(&list)) } else { apply_profile("start") };
    while let Ok(first) = rx.recv() {
        let mut reload = first == Event::Reload;
        loop {
            match rx.recv_timeout(SETTLE) {
                Ok(event) => reload |= event == Event::Reload,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ExitCode::FAILURE;
                }
            }
        }
        // O app Telas está mostrando uma prévia ("Manter ou desfazer"): ele cuida do resultado. O conjunto de
        // antes fica guardado: depois da prévia, só um monitor plugado ou tirado nesse meio tempo (o conjunto
        // diferente) ou um reload aplicam o perfil. Sem isso, a próxima mudança à mão seria desfeita
        if preview_active() {
            continue;
        }
        let Some(list) = sway::get_outputs() else { continue };
        let set = current_set(&list);
        if reload || last.as_ref() != Some(&set) {
            last = apply_profile(if reload { "reload" } else { "screens changed" });
        }
    }
    // O swaymsg terminou: o Sway saiu ou reiniciou o IPC. O laço do config decide se abre de novo
    let _ = child.kill();
    let _ = child.wait();
    ExitCode::FAILURE
}
