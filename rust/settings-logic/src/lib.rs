//! A Central de Configurações (swaydesk-settings, 0.2.0), sem GTK: a lista dos painéis, na ordem da barra
//! lateral, e os argumentos da linha de comando (`--panel <id>`).
//!
//! Os painéis próprios (Builtin) são montados pela Central; os de fora (External) só abrem um app que a distro
//! já traz (o nm-connection-editor, o Blueman, o pavucontrol) e aparecem só quando o programa está no PATH:
//! nenhum deles é dependência do pacote.

/// Marca um texto para o po/update (o `--keyword=n_` do xgettext) sem traduzi-lo: os títulos ficam numa tabela
/// const, e quem os mostra chama o `tr()` na hora.
pub mod about;
pub mod apps;
pub mod desktop;
pub mod input;
pub mod keyboard;
pub mod layers;
pub mod mouse;
pub mod power;
pub mod region;

pub const fn n_(text: &'static str) -> &'static str {
    text
}

/// O que um painel abre.
#[derive(Debug, PartialEq, Eq)]
pub enum Kind {
    /// Um painel da própria Central.
    Builtin,
    /// Um app de fora: o argv que o abre.
    External(&'static [&'static str]),
}

/// Um painel da barra lateral.
#[derive(Debug)]
pub struct PanelInfo {
    /// O nome do `--panel` e das ações do .desktop.
    pub id: &'static str,
    /// O título, em inglês (o msgid do po/).
    pub title: &'static str,
    /// O ícone, um nome do tema de ícones.
    pub icon: &'static str,
    pub kind: Kind,
    /// Outros nomes aceitos no `--panel`.
    pub aliases: &'static [&'static str],
    /// Um painel próprio que precisa de um programa de outro pacote (que a Central não exige): só aparece com
    /// ele no PATH.
    pub needs: Option<Need>,
}

/// O que um painel próprio precisa de fora: o programa que se procura no PATH e o pacote que o traz (o que o
/// aviso de "não instalado" manda instalar).
#[derive(Debug, PartialEq, Eq)]
pub struct Need {
    pub program: &'static str,
    pub package: &'static str,
}

/// Os painéis, na ordem da barra lateral.
pub const PANELS: &[PanelInfo] = &[
    // O Aparência aplica com os modelos do swaydesk-theme-tool (que traz o swaydesk-theme); o Telas precisa do
    // serviço de perfis do swaydesk-displays, que aplica o layout ao plugar uma tela
    builtin_needs("appearance", n_("Appearance"), "preferences-desktop-appearance", &["theme"], "swaydesk-theme", "swaydesk-theme-tool"),
    builtin_needs("displays", n_("Displays"), "preferences-desktop-display", &["screens", "monitors"], "swaydesk-displays", "swaydesk-displays"),
    builtin("keyboard", n_("Keyboard"), "input-keyboard", &[]),
    builtin("mouse", n_("Mouse and touchpad"), "input-mouse", &["touchpad"]),
    builtin("power", n_("Power and lock"), "system-shutdown", &["lock"]),
    builtin("desktop", n_("Desktop"), "user-desktop", &[]),
    builtin("default-apps", n_("Default apps"), "applications-other", &["apps"]),
    builtin("region", n_("Region"), "preferences-desktop-locale", &[]),
    builtin("about", n_("About"), "help-about", &[]),
    external("network", n_("Network"), "network-wired", &["nm-connection-editor"], &[]),
    external("bluetooth", n_("Bluetooth"), "bluetooth", &["blueman-manager"], &[]),
    external("sound", n_("Sound"), "audio-volume-high", &["pavucontrol"], &["audio"]),
];

const fn builtin(id: &'static str, title: &'static str, icon: &'static str, aliases: &'static [&'static str]) -> PanelInfo {
    PanelInfo { id, title, icon, kind: Kind::Builtin, aliases, needs: None }
}

const fn builtin_needs(
    id: &'static str,
    title: &'static str,
    icon: &'static str,
    aliases: &'static [&'static str],
    program: &'static str,
    package: &'static str,
) -> PanelInfo {
    PanelInfo { id, title, icon, kind: Kind::Builtin, aliases, needs: Some(Need { program, package }) }
}

const fn external(
    id: &'static str,
    title: &'static str,
    icon: &'static str,
    argv: &'static [&'static str],
    aliases: &'static [&'static str],
) -> PanelInfo {
    PanelInfo { id, title, icon, kind: Kind::External(argv), aliases, needs: None }
}

/// O painel de um nome do `--panel` (o id ou um apelido).
pub fn find(name: &str) -> Option<&'static PanelInfo> {
    PANELS.iter().find(|p| p.id == name || p.aliases.contains(&name))
}

/// O programa de que um painel precisa para aparecer (`needs`), pelo id ou apelido.
pub fn needs(name: &str) -> Option<&'static str> {
    find(name).and_then(|p| match p.kind {
        Kind::Builtin => p.needs.as_ref().map(|n| n.program),
        Kind::External(argv) => argv.first().copied(),
    })
}

/// O pacote que o aviso de "não instalado" de um painel manda instalar: o do programa de um painel próprio; o
/// próprio programa de um de fora (o nm-connection-editor, o blueman-manager e o pavucontrol têm o nome do
/// pacote, ou perto disso).
pub fn package_to_install(name: &str) -> Option<&'static str> {
    find(name).and_then(|p| match p.kind {
        Kind::Builtin => p.needs.as_ref().map(|n| n.package),
        Kind::External(argv) => argv.first().copied(),
    })
}

/// Os painéis que aparecem: os próprios sem `needs` sempre; os outros só se o programa existe (`installed`).
pub fn visible_panels(installed: impl Fn(&str) -> bool) -> Vec<&'static PanelInfo> {
    PANELS.iter().filter(|p| needs(p.id).is_none_or(&installed)).collect()
}

/// Se o programa está numa pasta do PATH dado (um arquivo executável), sem olhar o ambiente do processo.
pub fn on_path(program: &str, path: &std::ffi::OsStr) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // Só pastas absolutas: uma relativa (".", "bin") dependeria da pasta atual
    std::env::split_paths(path).filter(|dir| dir.is_absolute()).any(|dir| {
        std::fs::metadata(dir.join(program)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

/// O que a linha de comando pediu.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// O id do painel do `--panel`, se veio.
    pub panel: Option<&'static str>,
    /// `--about`: só imprime o relato do painel Sobre (o texto do "Copiar informações"), sem abrir a janela.
    pub about: bool,
}

/// Um erro na linha de comando (sai com 2).
#[derive(Debug, PartialEq, Eq)]
pub enum ArgError {
    UnknownPanel(String),
    MissingValue,
    UnknownArgument(String),
}

/// Lê os argumentos (sem o nome do programa).
pub fn parse_args(args: &[String]) -> Result<Options, ArgError> {
    let mut options = Options::default();
    // O --about vem sozinho: o que vier depois dele é o argumento a mais
    if args.first().is_some_and(|a| a == "--about") {
        return match args.get(1) {
            None => Ok(Options { about: true, ..options }),
            Some(extra) => Err(ArgError::UnknownArgument(extra.clone())),
        };
    }
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let name = if arg == "--panel" {
            rest.next().ok_or(ArgError::MissingValue)?
        } else if let Some(value) = arg.strip_prefix("--panel=") {
            value
        } else {
            return Err(ArgError::UnknownArgument(arg.clone()));
        };
        let panel = find(name).ok_or_else(|| ArgError::UnknownPanel(name.to_string()))?;
        options.panel = Some(panel.id);
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn the_builtin_panels_follow_the_plan_order() {
        let ids: Vec<&str> = PANELS.iter().filter(|p| p.kind == Kind::Builtin).map(|p| p.id).collect();
        assert_eq!(ids, ["appearance", "displays", "keyboard", "mouse", "power", "desktop", "default-apps", "region", "about"]);
    }

    #[test]
    fn network_bluetooth_and_sound_open_the_outside_apps() {
        let external: Vec<(&str, &[&str])> = PANELS
            .iter()
            .filter_map(|p| match p.kind {
                Kind::External(argv) => Some((p.id, argv)),
                Kind::Builtin => None,
            })
            .collect();
        assert_eq!(
            external,
            [("network", &["nm-connection-editor"][..]), ("bluetooth", &["blueman-manager"][..]), ("sound", &["pavucontrol"][..])]
        );
    }

    #[test]
    fn ids_are_unique_kebab_ascii_and_never_an_alias_of_another() {
        let mut names: Vec<&str> = Vec::new();
        for panel in PANELS {
            assert!(!panel.id.is_empty() && panel.id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'), "{}", panel.id);
            assert!(!panel.title.is_empty() && !panel.icon.is_empty(), "{}", panel.id);
            names.push(panel.id);
            names.extend(panel.aliases);
        }
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "a name twice: {names:?}");
    }

    #[test]
    fn a_panel_is_found_by_its_id_or_an_alias() {
        assert_eq!(find("displays").map(|p| p.id), Some("displays"));
        assert_eq!(find("theme").map(|p| p.id), Some("appearance"));
        assert_eq!(find("screens").map(|p| p.id), Some("displays"));
        assert_eq!(find("nope").map(|p| p.id), None);
    }

    #[test]
    fn outside_apps_show_only_when_installed() {
        let all: Vec<&str> = visible_panels(|_| true).iter().map(|p| p.id).collect();
        assert_eq!(all.len(), PANELS.len());
        let only_blueman: Vec<&str> = visible_panels(|p| p == "blueman-manager").iter().map(|p| p.id).collect();
        assert!(only_blueman.contains(&"bluetooth") && !only_blueman.contains(&"network") && !only_blueman.contains(&"sound"));
        assert!(only_blueman.contains(&"keyboard"), "a builtin panel without needs hidden");
    }

    #[test]
    fn appearance_and_displays_show_only_with_what_they_need() {
        // O Aparência aplica com os modelos do swaydesk-theme-tool, e o Telas precisa do serviço de perfis do
        // swaydesk-displays: a Central não depende desses pacotes, então os painéis somem sem eles
        assert_eq!(needs("appearance"), Some("swaydesk-theme"));
        assert_eq!(needs("displays"), Some("swaydesk-displays"));
        assert_eq!(needs("keyboard"), None);
        // O aviso de "não instalado" diz o pacote a instalar: o swaydesk-theme vem no swaydesk-theme-tool
        assert_eq!(package_to_install("appearance"), Some("swaydesk-theme-tool"));
        assert_eq!(package_to_install("theme"), Some("swaydesk-theme-tool"));
        assert_eq!(package_to_install("displays"), Some("swaydesk-displays"));
        assert_eq!(package_to_install("sound"), Some("pavucontrol"));
        assert_eq!(package_to_install("keyboard"), None);
        let none: Vec<&str> = visible_panels(|_| false).iter().map(|p| p.id).collect();
        assert!(!none.contains(&"appearance") && !none.contains(&"displays"), "{none:?}");
        assert!(none.contains(&"keyboard") && none.contains(&"about"), "{none:?}");
        let theme_only: Vec<&str> = visible_panels(|p| p == "swaydesk-theme").iter().map(|p| p.id).collect();
        assert!(theme_only.contains(&"appearance") && !theme_only.contains(&"displays"), "{theme_only:?}");
    }

    #[test]
    fn an_outside_panel_needs_only_its_own_program() {
        // O argv de um painel de fora já é o que ele precisa: um `needs` a mais seria uma segunda fonte
        for panel in PANELS {
            if let Kind::External(argv) = panel.kind {
                assert_eq!(panel.needs, None, "{}", panel.id);
                assert_eq!(needs(panel.id), argv.first().copied(), "{}", panel.id);
            }
        }
    }

    #[test]
    fn on_path_finds_an_executable_and_nothing_with_an_empty_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("settings-logic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("blueman-manager");
        std::fs::write(&exe, "").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plain = dir.join("pavucontrol");
        std::fs::write(&plain, "").unwrap();
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        let path = std::ffi::OsString::from(format!("/nonexistent:{}", dir.display()));
        let found = (on_path("blueman-manager", &path), on_path("pavucontrol", &path), on_path("blueman-manager", "".as_ref()));
        // Uma pasta relativa do PATH (".", "bin") dependeria da pasta atual: não conta
        let cwd = std::env::current_dir().unwrap();
        let relative = dir.strip_prefix(&cwd).map(|r| r.to_path_buf()).unwrap_or_else(|_| {
            std::path::PathBuf::from(format!("{}{}", "../".repeat(cwd.components().count()), dir.display().to_string().trim_start_matches('/')))
        });
        let from_relative = on_path("blueman-manager", relative.as_os_str());
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found, (true, false, false));
        assert!(!from_relative, "a relative PATH entry ({}) counted", relative.display());
    }

    #[test]
    fn the_command_line_takes_an_optional_panel() {
        assert_eq!(parse_args(&args(&[])), Ok(Options { panel: None, about: false }));
        // O relato do Sobre num terminal (para um bug com a janela fora do ar); sozinho, sem outro argumento
        assert_eq!(parse_args(&args(&["--about"])), Ok(Options { panel: None, about: true }));
        assert_eq!(parse_args(&args(&["--about", "--panel", "about"])), Err(ArgError::UnknownArgument("--panel".into())));
        assert_eq!(parse_args(&args(&["--panel", "about", "--about"])), Err(ArgError::UnknownArgument("--about".into())));
        assert_eq!(parse_args(&args(&["--panel", "displays"])), Ok(Options { panel: Some("displays"), about: false }));
        assert_eq!(parse_args(&args(&["--panel=keyboard"])), Ok(Options { panel: Some("keyboard"), about: false }));
        assert_eq!(parse_args(&args(&["--panel", "theme"])), Ok(Options { panel: Some("appearance"), about: false }));
    }

    #[test]
    fn a_bad_command_line_is_an_error() {
        assert_eq!(parse_args(&args(&["--panel", "nope"])), Err(ArgError::UnknownPanel("nope".into())));
        assert_eq!(parse_args(&args(&["--panel"])), Err(ArgError::MissingValue));
        assert_eq!(parse_args(&args(&["--frobnicate"])), Err(ArgError::UnknownArgument("--frobnicate".into())));
        assert_eq!(parse_args(&args(&["--panel", "displays", "extra"])), Err(ArgError::UnknownArgument("extra".into())));
    }
}
