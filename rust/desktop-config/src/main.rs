//! Lê a config do desktop e aplica a cor de destaque. O porte do `sway-desktop-config` em Python.
//!
//! uso: sway-desktop-config get <chave>   mostra um valor, para os scripts em Bash
//!      sway-desktop-config generate      grava os arquivos que dependem da cor
//!      sway-desktop-config apply         grava os arquivos e recarrega o Sway
//!      sway-desktop-config css           mostra as cores do destaque em CSS do GTK (@define-color)
//!      sway-desktop-config enabled <nome> sai com 0 se o componente está ligado no [enabled], 1 se não
//!      sway-desktop-config waybar [MODELO] grava o config da Waybar sem os módulos desligados e mostra o caminho
//!      sway-desktop-config keys          grava só o config.d/00-keys.conf (os includes do perfil de atalhos)
//!      sway-desktop-config setup         (só em Rust) cada login na sessão do pacote swaydesk-desktop: copia do
//!                                        skel o que falta no ~, acerta o teclado e o ~/.XCompose e faz o generate
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
    let install = if swaydesk_common::paths::dotfiles_install() {
        Install::Dotfiles
    } else if swaydesk_common::paths::package_session() {
        Install::PackageSession
    } else {
        Install::PlainSway
    };
    let waybar = (install == Install::PackageSession).then(|| swaydesk_common::paths::find_helper("sway-waybar")).flatten();
    after_apply_for(install, waybar.as_deref(), &Parts::installed())
}

/// Como a grade, o OSD e a Waybar sobem, que é o que o aviso do apply manda reiniciar
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Install {
    /// O binário no ~/.local/bin (o install.sh): os lançadores de lá
    Dotfiles,
    /// A sessão do pacote (SWAYDESK_SESSION=package): a grade e o OSD no laço do config do Sway, que os abre de
    /// novo (as units deles pulam nessa sessão), e a Waybar pelo sway-waybar do pacote
    PackageSession,
    /// Os pacotes num Sway puro: a grade e o OSD pelas units dos snippets, e a Waybar é a da pessoa
    PlainSway,
}

/// Os popups da barra (o waybar-NOME-popup dos dotfiles, o swaydesk-NOME-popup dos pacotes). O mesmo POPUPS do
/// desktop_config.py
const POPUPS: [&str; 11] =
    ["audio", "battery", "bluetooth", "brightness", "caffeine", "clipboard", "media", "network", "notifications", "system", "weather"];

/// O que está instalado do que muda de cor: cada componente é um pacote à parte (o app Tema vai sem nenhum deles)
#[derive(Clone, Copy, Debug)]
struct Parts {
    popups: bool,
    waybar: bool,
    drawer: bool,
    osd: bool,
    dock: bool,
    notifications: bool,
}

impl Parts {
    /// Cada um pelo lançador do ~/.local/bin (dotfiles) ou pelo binário ao lado deste ou no PATH (pacotes), como o
    /// paths::component acha; a Waybar, pelo programa no PATH. O mesmo do installed_parts do desktop_config.py
    fn installed() -> Parts {
        let has = |launcher: &str, binary: &str| swaydesk_common::paths::component(launcher, binary).is_file();
        let in_path = |name: &str| {
            std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(name).is_file()))
        };
        Parts {
            popups: POPUPS.iter().any(|n| has(&format!("waybar-{n}-popup"), &format!("swaydesk-{n}-popup"))),
            waybar: in_path("waybar"),
            drawer: has("sway-drawer", "swaydesk-drawer"),
            osd: has("sway-osd-bar", "swaydesk-osd-bar"),
            dock: has("sway-dock", "swaydesk-dock"),
            notifications: has("sway-notifd", "swaydesk-notifd"),
        }
    }
}

/// O aviso depois do apply, só com o que está instalado (vazio sem nada). Nos dotfiles, o sway-waybar de lá, como a
/// versão em Python; na sessão do pacote, o dele (sem ele, a Waybar muda no próximo login). Nos dois, o pkill que faz
/// o laço do config do Sway reabrir a grade e o OSD. No Sway puro, o try-restart das units (não liga uma parada). O comando fecha
/// cada linha: copiado, não leva a nota.
fn after_apply_for(install: Install, waybar: Option<&Path>, parts: &Parts) -> String {
    let again = tr("when you log in again");
    let reopens = tr("(it opens again by itself)");
    let mut lines: Vec<String> = Vec::new();
    let mut line = |label: String, note: Option<&str>, value: String| {
        lines.push(match note {
            Some(note) => format!("  {label} {note}: {value}"),
            None => format!("  {label}: {value}"),
        });
    };
    if parts.waybar {
        let value = match (install, waybar) {
            (Install::Dotfiles, _) => "~/.local/bin/sway-waybar restart".to_string(),
            (Install::PackageSession, Some(script)) => format!("{} restart", script.display()),
            _ => again.clone(),
        };
        line("Waybar".to_string(), None, value);
    }
    if parts.drawer {
        // Nos dotfiles e na sessão do pacote, a grade roda num laço do config do Sway, que a abre de novo: um start
        // a mais prenderia o terminal (é o residente) e disputaria a trava com o do laço
        match install {
            Install::PlainSway => line(tr("drawer"), None, "systemctl --user try-restart swaydesk-drawer.service".into()),
            _ => line(tr("drawer"), Some(&reopens), "pkill -u \"$USER\" -x sway-drawer".into()),
        }
    }
    if parts.osd {
        let indicator = tr("volume and brightness indicator");
        match install {
            Install::PlainSway => line(indicator, None, "systemctl --user try-restart swaydesk-osd.service".into()),
            _ => line(indicator, Some(&reopens), "pkill -u \"$USER\" -x sway-osd-bar".into()),
        }
    }
    if parts.dock {
        line(tr("dock"), None, again.clone());
    }
    if parts.notifications {
        line(tr("notifications"), None, again.clone());
    }
    let head = match (parts.popups, lines.is_empty()) {
        (false, true) => return String::new(),
        (true, true) => return tr("The popups already open with the new color."),
        (true, false) => tr("The popups already open with the new color. The rest changes when you log in again or restart it:"),
        (false, false) => tr("The new color shows when you log in again or restart it:"),
    };
    format!("{head}\n{}", lines.join("\n"))
}

/// Refaz o tema de ícones gerado antes dos settings.ini e do qt5ct, que o usam. Na sessão (apply), também
/// troca o nome no dconf. O generate pode rodar fora dela (o install.sh, num TTY): não mexe no dconf nem
/// apaga o tema gerado. Devolve None se não gravou os arquivos, Some(false) se só o dconf falhou.
fn write_files(config: &Config, session: bool) -> Option<bool> {
    let result = st::locked(|| -> Result<bool, Error> {
        if session {
            let ok = st::refresh_icons(config);
            dc::generate(config)?;
            // O cursor_theme e o tamanho na hora, no gsettings e na sessão (sem esperar o próximo login). O ambiente da
            // sessão que falha só avisa: os arquivos e o dconf foram gravados
            let (cursor_ok, env_ok) = st::refresh_cursor(config);
            if !env_ok {
                eprintln!("{}", fill_one(&tr("Could not update: %s"), &tr("cursor in the session environment")));
            }
            Ok(cursor_ok && ok)
        } else {
            st::config_icon_theme(config, false);
            dc::generate(config)?;
            Ok(true)
        }
    })
    .and_then(|r| r);
    // A Waybar sem os módulos desligados. Com ele quebrado, o sway-waybar usa o modelo. Sem modelo (o Sway puro sem
    // Waybar), nada a gerar nem a avisar
    let template = waybar::template_path();
    if result.is_ok() && template.is_file() {
        if let Err(err) = waybar::write(config, Some(&template)) {
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
    // A cor da tela de login (swaydesk-greeter), se ela estiver instalada
    swaydesk_common::accounts::set_own_accent(&config.accent);
    Some(ok)
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-desktop-config get KEY          prints a value of the desktop config\n",
    "       swaydesk-desktop-config generate       writes the files that depend on the accent color\n",
    "       swaydesk-desktop-config apply          writes them and reloads Sway\n",
    "       swaydesk-desktop-config css            prints the accent colors as GTK CSS\n",
    "       swaydesk-desktop-config enabled NAME   exits 0 if the component is on in [enabled], 1 if not\n",
    "       swaydesk-desktop-config waybar [MODEL] writes the Waybar config without the components turned off\n",
    "       swaydesk-desktop-config setup          at each login in the Swaydesk session",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--check") {
        return ExitCode::SUCCESS;
    }
    resident::set_process_name(PROGRAM);
    // As pastas sway-desktop → swaydesk: só no login (o setup antes do Sway, o sway-session) e no install.sh,
    // antes de ler a config; nunca no generate, no keys (o exec_always de cada reload) nem nos apps. O migrate
    // fica fora do usage, de propósito: não é para o usuário
    if matches!(args.as_slice(), [c] if c == "setup" || c == "migrate") {
        for message in swaydesk_common::dirs::migrate() {
            eprintln!("{PROGRAM}: {message}");
        }
        if args[0] == "migrate" {
            return ExitCode::SUCCESS;
        }
    }
    let config = config::load();
    // O cursor que o config pede e não vale: um tema que não está instalado, ou uma linha seat do config.d por cima
    if matches!(args.as_slice(), [c] if c == "generate" || c == "apply") {
        for warning in dc::cursor_warnings(&config) {
            eprintln!("{warning}");
        }
    }
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
            let hints = after_apply();
            if !hints.is_empty() {
                println!("{hints}");
            }
            if written { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        _ => {
            eprintln!("{}", usage());
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const ALL: Parts = Parts { popups: true, waybar: true, drawer: true, osd: true, dock: true, notifications: true };
    const NONE: Parts = Parts { popups: false, waybar: false, drawer: false, osd: false, dock: false, notifications: false };

    #[test]
    fn the_dotfiles_hints_name_the_launchers_in_local_bin() {
        let text = after_apply_for(Install::Dotfiles, None, &ALL);
        assert!(text.contains("~/.local/bin/sway-waybar restart"), "{text}");
        // A grade dos dotfiles roda no laço do config do Sway, como o OSD: o pkill basta, e um sway-drawer start a
        // mais prende o terminal (é o residente) e disputa a trava com o do laço (a rodada no mint22)
        assert!(!text.contains("sway-drawer start"), "{text}");
        assert!(text.lines().any(|l| l.ends_with(": pkill -u \"$USER\" -x sway-drawer")), "{text}");
    }

    // Na sessão do pacote, a grade e o OSD rodam no laço do config do Sway, não nas units (que pulam com o
    // SWAYDESK_SESSION): o systemctl restart não fazia nada, e o pkill faz o laço abrir de novo
    #[test]
    fn the_package_session_hints_name_its_sway_waybar_and_the_loops() {
        let text = after_apply_for(Install::PackageSession, Some(&PathBuf::from("/usr/lib/swaydesk/sway-waybar")), &ALL);
        assert!(!text.contains("~/.local/bin"), "{text}");
        assert!(!text.contains("systemctl"), "{text}");
        assert!(text.contains("Waybar: /usr/lib/swaydesk/sway-waybar restart"), "{text}");
    }

    // O comando fecha a linha em todo caso: copiado dali, não leva a nota junto (um parêntese é erro de sintaxe no
    // shell). E todo pkill leva o -u
    #[test]
    fn every_command_ends_its_line() {
        for install in [Install::Dotfiles, Install::PackageSession, Install::PlainSway] {
            let text = after_apply_for(install, Some(&PathBuf::from("/usr/lib/swaydesk/sway-waybar")), &ALL);
            for line in text.lines().filter(|l| l.contains("pkill") || l.contains("systemctl")) {
                assert!(!line.trim_end().ends_with(')'), "{install:?}: {line}");
            }
            for line in text.lines().filter(|l| l.contains("pkill")) {
                assert!(line.contains("pkill -u \"$USER\" -x"), "{install:?}: {line}");
            }
        }
    }

    #[test]
    fn without_the_sway_waybar_script_the_package_hint_says_log_in_again() {
        let text = after_apply_for(Install::PackageSession, None, &ALL);
        assert!(!text.contains("sway-waybar"), "{text}");
        assert!(text.contains("Waybar: when you log in again"), "{text}");
    }

    // No Sway puro com os pacotes, os snippets sobem a grade e o OSD pelas units, se a pessoa os ligou: o
    // try-restart não liga uma que está parada. A Waybar é a da pessoa
    #[test]
    fn on_a_plain_sway_the_hints_name_the_units() {
        let text = after_apply_for(Install::PlainSway, Some(&PathBuf::from("/usr/lib/swaydesk/sway-waybar")), &ALL);
        assert!(!text.contains("~/.local/bin"), "{text}");
        assert!(!text.contains("sway-waybar"), "{text}");
        assert!(text.contains("systemctl --user try-restart swaydesk-drawer.service"), "{text}");
        assert!(text.contains("systemctl --user try-restart swaydesk-osd.service"), "{text}");
    }

    // Só o app Tema (e o desktop-config) instalados: nada a reiniciar, nenhum popup, nenhuma linha
    #[test]
    fn with_only_the_theme_app_there_is_nothing_to_restart() {
        for install in [Install::Dotfiles, Install::PackageSession, Install::PlainSway] {
            assert_eq!(after_apply_for(install, None, &NONE), "", "{install:?}");
        }
    }

    // Tudo instalado: os popups e as seis linhas
    #[test]
    fn with_everything_each_part_has_its_line() {
        let text = after_apply_for(Install::PlainSway, None, &ALL);
        assert!(text.starts_with("The popups already open with the new color."), "{text}");
        for label in ["Waybar:", "drawer:", "volume and brightness indicator", "dock:", "notifications:"] {
            assert!(text.lines().any(|l| l.trim_start().starts_with(label)), "{label}: {text}");
        }
    }

    // Uma mistura (a grade e um popup, sem Waybar, OSD, dock nem notificações): só o que existe
    #[test]
    fn with_a_mix_only_the_installed_parts_are_named() {
        let parts = Parts { popups: true, drawer: true, ..NONE };
        let text = after_apply_for(Install::PlainSway, None, &parts);
        assert!(text.starts_with("The popups already open with the new color."), "{text}");
        assert!(text.contains("swaydesk-drawer.service"), "{text}");
        for absent in ["Waybar", "osd", "volume", "dock", "notifications"] {
            assert!(!text.contains(absent), "{absent}: {text}");
        }
        // Sem popups, a frase deles some, e o resto fica
        let parts = Parts { osd: true, ..NONE };
        let text = after_apply_for(Install::PackageSession, None, &parts);
        assert!(!text.contains("popups"), "{text}");
        assert!(text.lines().any(|l| l.ends_with(": pkill -u \"$USER\" -x sway-osd-bar")), "{text}");
        assert!(!text.contains("drawer"), "{text}");
    }
}
