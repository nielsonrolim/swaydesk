//! Popup da cafeína para o clique no ícone da Waybar: o porte de `home/.local/bin/waybar-caffeine-popup`,
//! que passa a vez a este binário (sway_rust).
//!
//! Com ela ligada não há bloqueio, tela apagada nem suspensão por inatividade; fechar a tampa continua
//! bloqueando. Quem controla é o sway-caffeine: aqui só há os botões. Clicar de novo no ícone, Esc,
//! clicar fora ou trocar de workspace fecha.
use std::cell::{Cell, RefCell};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::i18n::{ptr, tr};
use swaydesk_ui::cmd::{bin, spawn_detached, status};
use swaydesk_ui::list::{Options, CSS_OPT};
use swaydesk_ui::{label, run_popup, PopupWindow};

const PROGRAM: &str = "waybar-caffeine-popup";
const CSS: &str = ".detail { color: #8a8a8f; font-size: 11px; }\n";

/// O que o `sway-caffeine status` diz.
#[derive(Debug, PartialEq)]
enum State {
    Off,
    Forever,
    /// (quando desliga, em segundos desde 1970; a hora, HH:MM)
    Until(i64, String),
}

fn parse_status(text: &str) -> State {
    let parts: Vec<&str> = text.split_whitespace().collect();
    match parts.as_slice() {
        ["forever"] => State::Forever,
        ["until", epoch, time, ..] => match epoch.parse() {
            Ok(epoch) => State::Until(epoch, time.to_string()),
            Err(_) => State::Off,
        },
        _ => State::Off,
    }
}

/// O botão de um tempo ligado: o escolhido nesta janela ou, ao reabrir, o mais perto do que falta.
fn timed_button(picked: Option<&str>, until: i64, now: i64) -> String {
    picked.map(str::to_string).unwrap_or_else(|| if until - now <= 3600 { "1h" } else { "2h" }.into())
}

struct Popup {
    popup: Rc<PopupWindow>,
    options: Options,
    detail: gtk4::Label,
    /// 1 h ou 2 h escolhido aqui; ao reabrir, vale o tempo que falta
    picked: RefCell<Option<String>>,
    reading: Cell<bool>,
    loaded: Cell<bool>,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Rc<Popup> {
        let popup = PopupWindow::new(PROGRAM, "waybar-caffeine", &format!("{CSS_OPT}{CSS}"), true, main_loop);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        card.add_css_class("card");
        card.set_size_request(340, -1);
        popup.window.set_child(Some(&card));
        card.append(&label(&tr("Caffeine"), &["section"], 0.0));

        let this = Rc::new_cyclic(|me: &std::rc::Weak<Popup>| {
            let me = me.clone();
            let items = [
                ("off", ptr("caffeine", "Off")),
                ("1h", "1 h".to_string()),
                ("2h", "2 h".to_string()),
                ("on", tr("Always")),
            ];
            let options = Options::new(&items, move |key| {
                if let Some(p) = me.upgrade() {
                    p.on_pick(key);
                }
            });
            // Sempre com texto: o tamanho não muda, porque redimensionar faria o Sway tirar o foco
            let detail = label(" ", &["detail"], 0.0);
            Popup {
                popup: popup.clone(),
                options,
                detail,
                picked: RefCell::new(None),
                reading: Cell::new(false),
                loaded: Cell::new(false),
            }
        });
        card.append(&this.options.widget);
        card.append(&this.detail);
        card.append(&label(&tr("No lock, screen blanking or suspend when idle"), &["detail"], 0.0));

        this.refresh(); // a janela aparece quando a primeira leitura chegar
        let me = Rc::downgrade(&this);
        glib::timeout_add_seconds_local(2, move || match me.upgrade() {
            Some(p) => {
                p.refresh();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        this
    }

    /// Lê em outra linha: um status lento não pode travar os botões.
    fn refresh(self: &Rc<Self>) {
        if self.reading.replace(true) {
            return;
        }
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let text = gio::spawn_blocking(|| status(bin("sway-caffeine"), &["status"])).await.unwrap_or_default();
            if let Some(p) = me.upgrade() {
                p.reading.set(false);
                p.apply(parse_status(&text));
            }
        });
    }

    fn apply(&self, state: State) {
        match state {
            State::Until(until, time) => {
                let now = glib::real_time() / 1_000_000;
                self.options.select(&timed_button(self.picked.borrow().as_deref(), until, now));
                self.detail.set_text(&tr("Turns off at %s").replace("%s", &time));
            }
            State::Forever => {
                self.options.select("on");
                self.detail.set_text(&tr("On until you turn it off"));
            }
            State::Off => {
                self.picked.replace(None);
                self.options.select("off");
                self.detail.set_text(" ");
            }
        }
        if !self.loaded.replace(true) {
            self.popup.show();
        }
    }

    fn on_pick(self: &Rc<Self>, key: &str) {
        self.picked.replace(matches!(key, "1h" | "2h").then(|| key.to_string()));
        let _ = spawn_detached(bin("sway-caffeine"), &[key]);
        let me = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(200), move || {
            if let Some(p) = me.upgrade() {
                p.refresh();
            }
        });
    }
}

fn notify_icon() {
    // O ícone esconde a dica com o popup aberto
    swaydesk_common::resident::notify_icons("waybar-caffeine");
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-caffeine-popup em Python confere que o binário carrega
    }
    run_popup(PROGRAM, |main_loop| Some(Popup::new(main_loop)), notify_icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_of_sway_caffeine() {
        assert_eq!(parse_status("off"), State::Off);
        assert_eq!(parse_status("forever\n"), State::Forever);
        assert_eq!(parse_status("until 1800000000 14:20"), State::Until(1_800_000_000, "14:20".into()));
        assert_eq!(parse_status("until x 14:20"), State::Off);
        assert_eq!(parse_status(""), State::Off);
    }

    #[test]
    fn the_timed_button_follows_the_time_left() {
        assert_eq!(timed_button(None, 1000 + 3600, 1000), "1h");
        assert_eq!(timed_button(None, 1000 + 3601, 1000), "2h");
        assert_eq!(timed_button(Some("2h"), 1000 + 60, 1000), "2h");
    }
}
