//! Lê a config do desktop e aplica a cor de destaque. O porte do `sway-desktop-config` em Python.
//!
//! uso: sway-desktop-config get <chave>   mostra um valor, para os scripts em Bash
//!      sway-desktop-config generate      grava os arquivos que dependem da cor
//!      sway-desktop-config apply         grava os arquivos e recarrega o Sway
//!      sway-desktop-config css           mostra as cores do destaque em CSS do GTK (@define-color)
//!      sway-desktop-config enabled <nome> sai com 0 se o componente está ligado no [enabled], 1 se não
//!      sway-desktop-config waybar [MODELO] grava o config da Waybar sem os módulos desligados e mostra o caminho
//!      sway-desktop-config setup         (só em Rust) o primeiro login na sessão do pacote swaydesk-desktop:
//!                                        copia do skel o que falta no ~ e faz o generate
//! --check: só confere que o binário carrega.
mod waybar;

use std::path::Path;
use std::process::ExitCode;

use swaydesk_common::config::{self, css_colors, palette, Config};
use swaydesk_common::i18n::tr;
use swaydesk_common::resident;
use swaydesk_theme::{desktop as dc, fill_one, setup, themes as st, Error};

const PROGRAM: &str = "sway-desktop-config";

fn usage() -> String {
    tr("usage: sway-desktop-config get <key>   shows a value, for the Bash scripts\n       sway-desktop-config generate      writes the files that depend on the color\n       sway-desktop-config apply         writes the files and reloads Sway\n       sway-desktop-config css           shows the accent colors as GTK CSS (@define-color)\n       sway-desktop-config enabled NAME  exits 0 if the component is on in [enabled], 1 if not\n       sway-desktop-config waybar [FILE] writes the Waybar config without the modules turned off\n       sway-desktop-config keys          writes config.d/00-keys.conf, the includes of the shortcut profile")
}

fn after_apply() -> String {
    tr("The popups already open with the new color. The rest changes when you log in again or restart it:\n  Waybar: ~/.local/bin/sway-waybar restart\n  drawer: pkill -x sway-drawer; ~/.local/bin/sway-drawer start\n  volume and brightness indicator: pkill -x sway-osd-bar (it opens again by itself)\n  dock and notifications: when you log in again")
}

/// Refaz o tema de ícones gerado antes dos settings.ini e do qt5ct, que o usam. Na sessão (apply), também
/// troca o nome no dconf. O generate pode rodar fora dela (o install.sh, num TTY): não mexe no dconf nem
/// apaga o tema gerado. Devolve None se não gravou os arquivos, Some(false) se só o dconf falhou.
fn write_files(config: &Config, session: bool) -> Option<bool> {
    let result = st::locked(|| -> Result<bool, Error> {
        if session {
            let ok = st::refresh_icons(config);
            dc::generate(config)?;
            Ok(ok)
        } else {
            st::config_icon_theme(config, false);
            dc::generate(config)?;
            Ok(true)
        }
    })
    .and_then(|r| r);
    // A Waybar sem os módulos desligados. Sem o modelo, ou com ele quebrado, o sway-waybar usa o modelo
    if result.is_ok() {
        if let Err(err) = waybar::write(config, None) {
            eprintln!("{PROGRAM}: waybar config not generated: {err}");
        }
    }
    let ok = match result {
        Ok(ok) => ok,
        Err(err) => {
            eprintln!("{}", fill_one(&tr("could not write: %s"), &err.to_string()));
            return None;
        }
    };
    if !ok {
        eprintln!("{}", tr("Could not write the theme to dconf (gsettings)"));
    }
    Some(ok)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--check") {
        return ExitCode::SUCCESS;
    }
    resident::set_process_name(PROGRAM);
    let config = config::load();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["get", key] => match dc::get(&config, key) {
            Some(value) => {
                println!("{value}");
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("{}", fill_one(&tr("unknown key: %s"), key));
                ExitCode::from(2)
            }
        },
        // Para o sway-if-enabled: 0 ligado, 1 desligado. Um nome desconhecido avisa e conta como ligado
        ["enabled", name] => {
            if !config::known_component(name) {
                eprintln!("{PROGRAM}: unknown component: {name}");
            }
            if config.enabled(name) { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        ["waybar"] | ["waybar", _] => match waybar::write(&config, args.get(1).map(Path::new)) {
            Ok(path) => {
                println!("{}", path.display());
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("{PROGRAM}: waybar config not generated: {err}");
                ExitCode::from(1)
            }
        },
        // Só o config.d/00-keys.conf, para o sway-keys do início da sessão: rápido, sem o tema
        ["keys"] => {
            let file = dc::config_home().join("sway/config.d/00-keys.conf");
            match dc::write_file(&file, &dc::sway_keys(&config, &config::keys_dir())) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("{}", fill_one(&tr("could not write: %s"), &err.to_string()));
                    ExitCode::from(1)
                }
            }
        }
        ["css"] => {
            print!("{}", css_colors(&palette(&config)));
            ExitCode::SUCCESS
        }
        ["generate"] => if write_files(&config, false).is_some() { ExitCode::SUCCESS } else { ExitCode::from(1) },
        ["setup"] => {
            let keyboard = std::env::var_os("SWAY_DESKTOP_KEYBOARD").map(std::path::PathBuf::from).unwrap_or_else(|| "/etc/default/keyboard".into());
            if let Err(err) = setup::setup(&setup::skel_dir(), &dc::home(), &keyboard) {
                eprintln!("{}", fill_one(&tr("could not write: %s"), &err.to_string()));
                return ExitCode::from(1);
            }
            // A config.toml pode ter acabado de chegar do skel
            if write_files(&config::load(), false).is_some() { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        ["apply"] => {
            let Some(written) = write_files(&config, true) else { return ExitCode::from(1) };
            // Os arquivos já mudaram: o Sway recarrega mesmo que o dconf tenha falhado
            if !dc::reload_sway() {
                eprintln!("{}", tr("Sway did not reload: run swaymsg reload in the session."));
            }
            println!("{}", after_apply());
            if written { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        _ => {
            eprintln!("{}", usage());
            ExitCode::from(2)
        }
    }
}
