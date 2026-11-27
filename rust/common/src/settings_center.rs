//! A Central de Configurações (swaydesk-settings) vista pelos outros componentes: o popup Sistema e a Ajuda
//! mostram "Configurações" e o Tema e as Telas abrem o painel dela só quando ela está instalada. Nenhum pacote
//! depende dela: sem o programa no PATH, a entrada some e cada app abre sozinho. O `sway_settings_center.py`
//! espelha isto.
use std::ffi::OsStr;
use std::path::PathBuf;

/// O programa da Central.
pub const PROGRAM: &str = "swaydesk-settings";

/// A opção do .desktop do Tema e das Telas: com a Central instalada, o app passa a vez ao painel dela.
pub const SETTINGS_PANEL_FLAG: &str = "--settings-panel";

/// O caminho do programa na primeira pasta absoluta do PATH dado em que ele é um arquivo executável, sem olhar o
/// ambiente do processo. É esse caminho que se executa, e não o nome: o exec pelo nome leria o PATH todo, e uma pasta
/// relativa (".", "bin"), que dependeria da pasta atual e que esta busca pula, poderia ganhar.
pub fn find_on_path(program: &str, path: &OsStr) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path).filter(|dir| dir.is_absolute()).map(|dir| dir.join(program)).find(|candidate| {
        std::fs::metadata(candidate).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

/// Se o programa está numa pasta absoluta do PATH dado (um arquivo executável), como o `find_on_path`.
pub fn on_path(program: &str, path: &OsStr) -> bool {
    find_on_path(program, path).is_some()
}

/// O caminho absoluto da Central no PATH dado.
pub fn find_in(path: &OsStr) -> Option<PathBuf> {
    find_on_path(PROGRAM, path)
}

/// O caminho absoluto da Central no PATH do processo.
pub fn find() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| find_in(&path))
}

/// Se a Central está no PATH dado.
pub fn installed_in(path: &OsStr) -> bool {
    find_in(path).is_some()
}

/// Se a Central está no PATH do processo.
pub fn installed() -> bool {
    find().is_some()
}

/// Se a sessão é um Sway pelo XDG_CURRENT_DESKTOP (a lista separada por ":"), como o OnlyShowIn=sway da Central:
/// os painéis dela falam com o Sway, e o Tema (que também aparece no Hyprland, no river e no niri) só passa a vez a
/// ela dentro do Sway.
pub fn in_sway(current_desktop: &str) -> bool {
    current_desktop.split(':').any(|name| name.eq_ignore_ascii_case("sway"))
}

/// Os argumentos que abrem a Central (sem o programa, o PROGRAM), no painel dado ou na primeira tela dela.
pub fn args(panel: Option<&str>) -> Vec<&str> {
    match panel {
        Some(id) => vec!["--panel", id],
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    /// Uma pasta só deste teste, com um arquivo de cada nome e modo dados.
    fn bin_dir(test: &str, files: &[(&str, u32)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swaydesk-settings-center-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, mode) in files {
            std::fs::write(dir.join(name), "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(dir.join(name), std::fs::Permissions::from_mode(*mode)).unwrap();
        }
        dir
    }

    #[test]
    fn the_center_is_found_only_as_an_executable_on_the_path() {
        let exe = bin_dir("exe", &[(PROGRAM, 0o755)]);
        let plain = bin_dir("plain", &[(PROGRAM, 0o644)]);
        let other = bin_dir("other", &[("swaydesk-theme-app", 0o755)]);
        let path = std::ffi::OsString::from(format!("/nonexistent:{}", exe.display()));
        let found = (installed_in(&path), installed_in(plain.as_os_str()), installed_in(other.as_os_str()), installed_in("".as_ref()));
        for dir in [exe, plain, other] {
            let _ = std::fs::remove_dir_all(dir);
        }
        // Um arquivo sem o x não conta, nem outro programa, nem um PATH vazio
        assert_eq!(found, (true, false, false, false));
    }

    #[test]
    fn a_relative_path_entry_is_ignored() {
        let dir = bin_dir("relative", &[("blueman-manager", 0o755)]);
        // Uma pasta relativa do PATH (".", "bin") dependeria da pasta atual: não conta
        let cwd = std::env::current_dir().unwrap();
        let relative = dir.strip_prefix(&cwd).map(Path::to_path_buf).unwrap_or_else(|_| {
            PathBuf::from(format!("{}{}", "../".repeat(cwd.components().count()), dir.display().to_string().trim_start_matches('/')))
        });
        let (absolute, from_relative) = (on_path("blueman-manager", dir.as_os_str()), on_path("blueman-manager", relative.as_os_str()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(absolute);
        assert!(!from_relative, "a relative PATH entry ({}) counted", relative.display());
    }

    #[test]
    fn the_found_program_is_the_absolute_one_even_behind_a_relative_entry() {
        // "." (ou outra pasta relativa) na frente do PATH com um swaydesk-settings nela: a busca pula a relativa e
        // devolve o caminho absoluto da pasta de verdade, que é o que se executa (o exec pelo nome leria o PATH todo
        // e pegaria o da pasta atual)
        let fake = bin_dir("found-fake", &[(PROGRAM, 0o755)]);
        let real = bin_dir("found-real", &[(PROGRAM, 0o755)]);
        let cwd = std::env::current_dir().unwrap();
        let relative = fake.strip_prefix(&cwd).map(Path::to_path_buf).unwrap_or_else(|_| {
            PathBuf::from(format!("{}{}", "../".repeat(cwd.components().count()), fake.display().to_string().trim_start_matches('/')))
        });
        let path = std::ffi::OsString::from(format!("{}:{}", relative.display(), real.display()));
        let found = (find_in(&path), find_in(fake.as_os_str()), find_in("".as_ref()), find_on_path("blueman-manager", &path));
        let expected = real.join(PROGRAM);
        let fake_program = fake.join(PROGRAM);
        for dir in [fake, real] {
            let _ = std::fs::remove_dir_all(dir);
        }
        assert_eq!(found.0, Some(expected));
        assert!(found.0.as_ref().is_some_and(|p| p.is_absolute()));
        assert_eq!(found.1, Some(fake_program));
        assert_eq!((found.2, found.3), (None, None));
    }

    #[test]
    fn only_a_sway_session_counts() {
        let got: Vec<bool> = ["sway", "SWAY", "sway:wlroots", "ubuntu:sway", "Hyprland", "niri", "river", "", "swayfx"]
            .into_iter()
            .map(in_sway)
            .collect();
        assert_eq!(got, [true, true, true, true, false, false, false, false, false]);
    }

    #[test]
    fn the_args_open_a_panel_or_the_first_screen() {
        assert_eq!(args(Some("appearance")), ["--panel", "appearance"]);
        assert!(args(None).is_empty());
    }

    #[test]
    fn the_names_match_the_python_mirror() {
        let python = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../home/.local/bin/sway_settings_center.py");
        // O espelho em Python só existe no repositório (não no tarball dos pacotes)
        let Ok(python) = std::fs::read_to_string(python) else { return };
        assert!(python.contains(&format!("PROGRAM = \"{PROGRAM}\"")));
        assert!(python.contains(&format!("SETTINGS_PANEL_FLAG = \"{SETTINGS_PANEL_FLAG}\"")));
    }
}
