//! As sessões Wayland instaladas (`wayland-sessions/*.desktop` dos `XDG_DATA_DIRS`), como os
//! gerenciadores de login as listam, e o comando e o ambiente que o greetd recebe para abrir uma delas.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A sessão do pacote swaydesk-desktop: vem primeiro na lista.
pub const OURS: &str = "swaydesk";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// O nome do arquivo sem o `.desktop`: é o que a memória guarda
    pub id: String,
    /// O Name traduzido, se houver
    pub name: String,
    /// O Exec já separado, sem os códigos de campo (%f, %u...)
    pub cmd: Vec<String>,
    /// O DesktopNames (o XDG_CURRENT_DESKTOP da sessão), separado por ':'
    pub desktop_names: String,
}

impl Session {
    /// O ambiente que o greetd passa à sessão, além do que ele já põe (o do PAM e o do usuário).
    pub fn env(&self) -> Vec<String> {
        let mut env = vec!["XDG_SESSION_TYPE=wayland".to_string(), format!("XDG_SESSION_DESKTOP={}", self.id)];
        if !self.desktop_names.is_empty() {
            env.push(format!("XDG_CURRENT_DESKTOP={}", self.desktop_names));
        }
        env
    }
}

/// As pastas de sessões, na ordem de prioridade: os XDG_DATA_DIRS (padrão /usr/local/share:/usr/share).
pub fn session_dirs(data_dirs: Option<&str>) -> Vec<PathBuf> {
    let dirs = data_dirs.filter(|d| !d.is_empty()).unwrap_or("/usr/local/share:/usr/share");
    dirs.split(':').filter(|d| d.starts_with('/')).map(|d| Path::new(d).join("wayland-sessions")).collect()
}

/// As chaves do grupo [Desktop Entry] de um .desktop.
fn entry_keys(text: &str) -> Vec<(String, String)> {
    let mut keys = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[Desktop Entry]";
            continue;
        }
        if !inside || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            keys.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    keys
}

/// O valor de uma chave traduzível: Name[pt_BR], depois Name[pt], depois Name.
fn localized(keys: &[(String, String)], key: &str, locale: &str) -> Option<String> {
    let base = locale.split('.').next().unwrap_or("").split('@').next().unwrap_or("");
    let lang = base.split('_').next().unwrap_or("");
    let mut wanted = Vec::new();
    if !base.is_empty() {
        wanted.push(format!("{key}[{base}]"));
    }
    if !lang.is_empty() && lang != base {
        wanted.push(format!("{key}[{lang}]"));
    }
    wanted.push(key.to_string());
    wanted.iter().find_map(|w| keys.iter().find(|(k, _)| k == w).map(|(_, v)| v.clone())).filter(|v| !v.is_empty())
}

/// O Exec em palavras, como o shell separaria, sem os códigos de campo (%%  vira %). None se as aspas
/// estiverem erradas ou se não sobrar comando.
pub fn split_exec(exec: &str) -> Option<Vec<String>> {
    let words = shlex::split(exec)?;
    let cmd: Vec<String> = words
        .into_iter()
        .filter(|w| !(w.len() == 2 && w.starts_with('%') && w != "%%"))
        .map(|w| w.replace("%%", "%"))
        .collect();
    (!cmd.is_empty()).then_some(cmd)
}

/// Um programa existe: um caminho executável, ou um nome achado no PATH.
fn program_exists(program: &str, path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let executable = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if program.contains('/') {
        return executable(Path::new(program));
    }
    path.split(':').filter(|d| !d.is_empty()).any(|dir| executable(&Path::new(dir).join(program)))
}

/// Lê uma sessão. None para as ocultas (Hidden, NoDisplay), as sem Exec válido e as de TryExec ausente.
pub fn read_session(file: &Path, locale: &str, path: &str) -> Option<Session> {
    let text = std::fs::read_to_string(file).ok()?;
    let keys = entry_keys(&text);
    let get = |key: &str| keys.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    if get("Hidden") == Some("true") || get("NoDisplay") == Some("true") {
        return None;
    }
    if let Some(try_exec) = get("TryExec").filter(|t| !t.is_empty()) {
        if !program_exists(try_exec, path) {
            return None;
        }
    }
    let cmd = split_exec(get("Exec")?)?;
    let id = file.file_stem()?.to_str()?.to_string();
    let name = localized(&keys, "Name", locale).unwrap_or_else(|| id.clone());
    let desktop_names = get("DesktopNames").unwrap_or("").split(';').filter(|n| !n.is_empty()).collect::<Vec<_>>().join(":");
    Some(Session { id, name, cmd, desktop_names })
}

/// Todas as sessões: um id repetido vale o da pasta que vem antes. A nossa primeiro, o resto pelo nome.
pub fn sessions(dirs: &[PathBuf], locale: &str, path: &str) -> Vec<Session> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "desktop")).collect();
        files.sort();
        for file in files {
            let Some(id) = file.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
            if !seen.insert(id) {
                continue;
            }
            if let Some(session) = read_session(&file, locale, path) {
                found.push(session);
            }
        }
    }
    found.sort_by_key(|s| (s.id != OURS, s.name.to_lowercase()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swaydesk-greeter-sessions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn exec_is_split_like_the_shell_without_field_codes() {
        assert_eq!(split_exec("sway --unsupported-gpu"), Some(vec!["sway".into(), "--unsupported-gpu".into()]));
        assert_eq!(split_exec("\"/opt/my wm/start\" %f --x=100%%"), Some(vec!["/opt/my wm/start".into(), "--x=100%".into()]));
        assert_eq!(split_exec("sway \"unclosed"), None);
        assert_eq!(split_exec("%U"), None);
    }

    #[test]
    fn sessions_are_listed_ours_first_and_translated() {
        let local = tempdir("local");
        let usr = tempdir("usr");
        std::fs::write(usr.join("swaydesk.desktop"), "[Desktop Entry]\nName=Sway Desktop\nExec=/usr/lib/swaydesk/sway-session --package\nDesktopNames=sway\n").unwrap();
        std::fs::write(usr.join("gnome.desktop"), "[Desktop Entry]\nName=GNOME\nName[pt_BR]=GNOME em português\nExec=gnome-session\nDesktopNames=GNOME;\n").unwrap();
        std::fs::write(usr.join("hidden.desktop"), "[Desktop Entry]\nName=Hidden\nExec=x\nHidden=true\n").unwrap();
        std::fs::write(usr.join("missing.desktop"), "[Desktop Entry]\nName=Missing\nExec=x\nTryExec=/nao/existe\n").unwrap();
        std::fs::write(usr.join("broken.desktop"), "[Desktop Entry]\nName=Broken\n").unwrap();
        std::fs::write(usr.join("Alpha.desktop"), "[Other]\nExec=nope\n[Desktop Entry]\nName=Alpha\nExec=alpha\n").unwrap();
        // A pasta que vem antes vence o mesmo id
        std::fs::write(local.join("gnome.desktop"), "[Desktop Entry]\nName=GNOME local\nExec=gnome-local\n").unwrap();
        std::fs::write(local.join("notes.txt"), "x").unwrap();
        let list = sessions(&[local.clone(), usr.clone(), PathBuf::from("/nao/existe")], "pt_BR.UTF-8", "");
        let ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["swaydesk", "Alpha", "gnome"]);
        assert_eq!(list[2].name, "GNOME local");
        assert_eq!(list[2].cmd, ["gnome-local"]);
        assert_eq!(list[0].env(), ["XDG_SESSION_TYPE=wayland", "XDG_SESSION_DESKTOP=swaydesk", "XDG_CURRENT_DESKTOP=sway"]);
        let gnome = read_session(&usr.join("gnome.desktop"), "pt_BR.UTF-8", "").unwrap();
        assert_eq!(gnome.name, "GNOME em português");
        assert_eq!(gnome.desktop_names, "GNOME");
        assert_eq!(read_session(&usr.join("gnome.desktop"), "C", "").unwrap().name, "GNOME");
        let _ = std::fs::remove_dir_all(&local);
        let _ = std::fs::remove_dir_all(&usr);
    }

    #[test]
    fn try_exec_looks_in_the_path() {
        let dir = tempdir("tryexec");
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("labwc"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(bin.join("labwc"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("labwc.desktop"), "[Desktop Entry]\nName=labwc\nExec=labwc\nTryExec=labwc\n").unwrap();
        assert!(read_session(&dir.join("labwc.desktop"), "", bin.to_str().unwrap()).is_some());
        assert!(read_session(&dir.join("labwc.desktop"), "", "/nao/existe").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_session_dirs_follow_xdg_data_dirs() {
        assert_eq!(session_dirs(None), [PathBuf::from("/usr/local/share/wayland-sessions"), PathBuf::from("/usr/share/wayland-sessions")]);
        assert_eq!(session_dirs(Some("/opt/share:relative:")), [PathBuf::from("/opt/share/wayland-sessions")]);
    }
}
