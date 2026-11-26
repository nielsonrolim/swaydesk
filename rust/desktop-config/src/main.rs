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
    after_apply_for(install, waybar.as_deref())
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

/// O aviso depois do apply. Nos dotfiles, o mesmo texto da versão em Python. Na sessão do pacote, o sway-waybar dele
/// (sem ele, a Waybar muda no próximo login) e o pkill que faz o laço reabrir a grade e o OSD. No Sway puro, as units.
fn after_apply_for(install: Install, waybar: Option<&Path>) -> String {
    match install {
        Install::Dotfiles => tr("The popups already open with the new color. The rest changes when you log in again or restart it:\n  Waybar: ~/.local/bin/sway-waybar restart\n  drawer: pkill -x sway-drawer; ~/.local/bin/sway-drawer start\n  volume and brightness indicator: pkill -x sway-osd-bar (it opens again by itself)\n  dock and notifications: when you log in again"),
        Install::PackageSession => {
            let waybar = match waybar {
                Some(script) => format!("{} restart", script.display()),
                None => tr("when you log in again"),
            };
            fill_one(&tr("The popups already open with the new color. The rest changes when you log in again or restart it:\n  Waybar: %s\n  drawer (it opens again by itself): pkill -u \"$USER\" -x sway-drawer\n  volume and brightness indicator (it opens again by itself): pkill -u \"$USER\" -x sway-osd-bar\n  dock and notifications: when you log in again"), &waybar)
        }
        Install::PlainSway => tr("The popups already open with the new color. The rest changes when you log in again or restart it:\n  Waybar: when you log in again\n  drawer: systemctl --user restart swaydesk-drawer.service\n  volume and brightness indicator: systemctl --user restart swaydesk-osd.service\n  dock and notifications: when you log in again"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_dotfiles_hints_name_the_launchers_in_local_bin() {
        let text = after_apply_for(Install::Dotfiles, None);
        assert!(text.contains("~/.local/bin/sway-waybar restart"), "{text}");
        assert!(text.contains("~/.local/bin/sway-drawer start"), "{text}");
    }

    // Na sessão do pacote, a grade e o OSD rodam no laço do config do Sway, não nas units (que pulam com o
    // SWAYDESK_SESSION): o systemctl restart não fazia nada, e o pkill faz o laço abrir de novo
    #[test]
    fn the_package_session_hints_name_its_sway_waybar_and_the_loops() {
        let text = after_apply_for(Install::PackageSession, Some(&PathBuf::from("/usr/lib/swaydesk/sway-waybar")));
        assert!(!text.contains("~/.local/bin"), "{text}");
        assert!(!text.contains("systemctl"), "{text}");
        assert!(text.contains("Waybar: /usr/lib/swaydesk/sway-waybar restart"), "{text}");
        // O comando fecha a linha: copiado dali, não leva a nota junto (um parêntese é erro de sintaxe no shell)
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.iter().any(|l| l.ends_with(": pkill -u \"$USER\" -x sway-drawer")), "{text}");
        assert!(lines.iter().any(|l| l.ends_with(": pkill -u \"$USER\" -x sway-osd-bar")), "{text}");
    }

    #[test]
    fn without_the_sway_waybar_script_the_package_hint_says_log_in_again() {
        let text = after_apply_for(Install::PackageSession, None);
        assert!(!text.contains("sway-waybar"), "{text}");
        assert!(text.contains("Waybar: when you log in again"), "{text}");
    }

    // No Sway puro com os pacotes, os snippets sobem a grade e o OSD pelas units; a Waybar é a da pessoa
    #[test]
    fn on_a_plain_sway_the_hints_name_the_units() {
        let text = after_apply_for(Install::PlainSway, Some(&PathBuf::from("/usr/lib/swaydesk/sway-waybar")));
        assert!(!text.contains("~/.local/bin"), "{text}");
        assert!(!text.contains("sway-waybar"), "{text}");
        assert!(text.contains("systemctl --user restart swaydesk-drawer.service"), "{text}");
        assert!(text.contains("systemctl --user restart swaydesk-osd.service"), "{text}");
    }
}
