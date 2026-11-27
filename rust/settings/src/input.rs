//! O que os painéis Teclado e Mouse e touchpad dividem: o 80-input.conf (gravado pelo sway_conf, que recusa um
//! config.d ligado a um repositório git), o que o Sway diz agora (GET_INPUTS, GET_CONFIG) e a aplicação ao vivo
//! pelo `swaymsg`, uma por vez (o Serial): as chaves mudadas durante uma aplicação se juntam e vão na seguinte.
//! As regras são do swaydesk-settings-logic (input, keyboard, mouse).
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::i18n::tr;
use swaydesk_common::sway_conf;
use swaydesk_settings_logic::desktop::Serial;
use swaydesk_settings_logic::input::{self as logic, Device, InputConf, FILE_NAME};
use swaydesk_settings_logic::mouse::includes;

use crate::desktop::{read, with};

pub(crate) fn file() -> PathBuf {
    sway_conf::config_d_file(FILE_NAME)
}

/// Se há um Sway para falar com ele agora.
fn has_sway() -> bool {
    std::env::var_os("SWAYSOCK").is_some_and(|s| !s.is_empty())
}

/// A saída do `swaymsg` com estes argumentos, se ele saiu bem.
fn swaymsg(args: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new("swaymsg");
    command.args(args).stdin(std::process::Stdio::null());
    let output = swaydesk_ui::cmd::child_env(&mut command).output().map_err(|err| format!("swaymsg: {err}"))?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(text)
    } else {
        // Um comando recusado: o swaymsg imprime a lista de resultados, cada um com o "error" dele
        let errors: Vec<String> = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|r| r.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .collect();
        let err = String::from_utf8_lossy(&output.stderr);
        Err(match errors.is_empty() {
            false => errors.join("\n"),
            true if !err.trim().is_empty() => format!("swaymsg: {}", err.trim()),
            true => format!("swaymsg: {}", text.trim()),
        })
    }
}

/// O que o Sway diz agora: os dispositivos e o config principal (None sem Sway).
#[derive(Clone, Debug, Default)]
pub(crate) struct Snapshot {
    pub inputs: serde_json::Value,
    pub main_config: Option<String>,
}

/// Lê o Sway (bloqueia: rodar no gio::spawn_blocking).
pub(crate) fn snapshot() -> Snapshot {
    if !has_sway() {
        return Snapshot::default();
    }
    let json = |args: &[&str]| swaymsg(args).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let inputs = json(&["-r", "-t", "get_inputs"]).unwrap_or_default();
    let main_config = json(&["-r", "-t", "get_config"])
        .and_then(|v| v.get("config").and_then(|c| c.as_str()).map(str::to_string));
    Snapshot { inputs, main_config }
}

/// Os arquivos do config.d na ordem do `include .../config.d/*` (a do glob).
fn config_d() -> Vec<String> {
    let Some(dir) = file().parent().map(PathBuf::from) else { return Vec::new() };
    let mut names: Vec<PathBuf> =
        std::fs::read_dir(dir).map(|e| e.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    names.sort();
    // Como o glob do Sway: sem os arquivos ocultos; e sem o temporário de uma gravação que caiu no meio
    let loaded = |p: &PathBuf| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        p.is_file() && !name.starts_with('.') && !name.ends_with(".tmp")
    };
    names.iter().filter(|p| loaded(p)).filter_map(|p| std::fs::read_to_string(p).ok()).collect()
}

/// As chaves que valem: o config principal e o config.d.
pub(crate) fn effective(main_config: Option<&str>) -> InputConf {
    let mut texts: Vec<String> = main_config.map(str::to_string).into_iter().collect();
    texts.extend(config_d());
    InputConf::effective(&texts)
}

/// O nosso arquivo, como está.
pub(crate) fn ours() -> InputConf {
    InputConf::parse(&read(&file()).unwrap_or_default())
}

/// Os avisos embaixo do painel (`panel`: os tipos dele): um bloco de um dispositivo só que vale mais que o painel
/// para ele, e um config do Sway que pode não ler o arquivo no login. None: nada a dizer.
pub(crate) fn notes(snapshot: &Snapshot, panel: &[Device]) -> Option<String> {
    let mut texts: Vec<String> = snapshot.main_config.iter().cloned().collect();
    texts.extend(config_d());
    let devices = InputConf::device_specific(&texts, panel);
    let mut out = Vec::new();
    if !devices.is_empty() {
        out.push(with(
            tr("A setting for a single device in your Sway config wins over this panel for it: %(devices)s"),
            "devices",
            &devices.join(", "),
        ));
    }
    out.extend(include_hint(snapshot));
    (!out.is_empty()).then(|| out.join("\n\n"))
}

/// A dica de um Sway cujo config pode não ler o arquivo no login (None: lê, ou não há Sway para perguntar).
fn include_hint(snapshot: &Snapshot) -> Option<String> {
    let main = snapshot.main_config.as_deref()?;
    let file = file();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let config_dir = file.parent().and_then(|d| d.parent()).map(PathBuf::from).unwrap_or_default();
    if includes(main, &file, &home, &config_dir) {
        return None;
    }
    let line = format!("include {}", file.display());
    Some(with(
        tr("Your Sway config may not read these settings at login. To keep them, add this line to it:\n%(line)s"),
        "line",
        &line,
    ))
}

/// A gravação e a aplicação ao vivo de um painel.
pub(crate) struct Applier {
    serial: RefCell<Serial>,
    pending: RefCell<Vec<(Device, &'static str)>>,
    /// Onde vai o resultado ("vale agora", "no próximo login", o erro)
    pub status: gtk4::Label,
}

impl Applier {
    pub fn new() -> Rc<Applier> {
        let status = gtk4::Label::new(None);
        status.add_css_class("dim-label");
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        status.set_max_width_chars(70);
        status.set_visible(false);
        Rc::new(Applier { serial: RefCell::default(), pending: RefCell::default(), status })
    }

    fn say(&self, text: &str, error: bool) {
        self.status.set_text(text);
        if error {
            self.status.add_css_class("error");
        } else {
            self.status.remove_css_class("error");
        }
        self.status.set_visible(true);
    }

    /// Muda o nosso arquivo (`edit`), grava e aplica as chaves `keys` ao vivo. Erro de gravação: nada muda.
    pub fn change(self: &Rc<Self>, keys: &[(Device, &'static str)], edit: impl FnOnce(&mut InputConf)) -> bool {
        let mut conf = ours();
        edit(&mut conf);
        if let Err(err) = sway_conf::save(&file(), &conf.render()) {
            self.say(&with(tr("Could not save:\n%(error)s"), "error", &err.to_string()), true);
            return false;
        }
        // Sem Sway, ou só o Num Lock (que o Sway só lê no config): vale no próximo login
        if !has_sway() || keys.iter().all(|(_, key)| *key == "xkb_numlock") {
            self.say(&tr("Saved. Changes apply at the next login"), false);
            return true;
        }
        {
            let mut pending = self.pending.borrow_mut();
            for key in keys {
                if !pending.contains(key) {
                    pending.push(*key);
                }
            }
        }
        if self.serial.borrow_mut().request() {
            self.spawn();
        }
        true
    }

    fn spawn(self: &Rc<Self>) {
        let keys = std::mem::take(&mut *self.pending.borrow_mut());
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let snapshot = snapshot();
                let command = logic::apply(&effective(snapshot.main_config.as_deref()), &keys, |name| {
                    std::env::var(name).ok()
                });
                if command.is_empty() {
                    return Ok(());
                }
                swaymsg(&[&command]).map(|_| ())
            })
            .await
            .unwrap_or_else(|_| Err("swaymsg".into()));
            let Some(applier) = weak.upgrade() else { return };
            match result {
                Ok(()) => applier.say(&tr("Changes apply at once"), false),
                Err(err) => applier.say(&with(tr("Saved, but not applied:\n%(error)s"), "error", &err), true),
            }
            if applier.serial.borrow_mut().finished() {
                applier.spawn();
            }
        });
    }
}
