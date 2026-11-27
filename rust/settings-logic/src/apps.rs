//! O painel Apps padrão, sem GTK: as categorias, com os tipos MIME de cada uma, e o editor do
//! `~/.config/mimeapps.list` (só a camada do usuário: o /etc/xdg, onde as ISOs põem o Loupe e o Papers, nunca é
//! escrito). O gerenciador de arquivos e o terminal não passam por aqui: são as chaves file_manager e terminal do
//! config.toml (o `sway-mimeapps.list` gerado, que leva o inode/directory, vence o mimeapps.list da mesma pasta).
use std::path::PathBuf;

use crate::n_;

/// Uma categoria do painel: os tipos que ela muda juntos, o primeiro sendo o que decide os candidatos e o padrão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    pub types: &'static [&'static str],
}

pub const CATEGORIES: &[Category] = &[
    Category {
        id: "web",
        name: n_("Web browser"),
        types: &["x-scheme-handler/http", "x-scheme-handler/https", "text/html", "application/xhtml+xml"],
    },
    Category { id: "mail", name: n_("Mail"), types: &["x-scheme-handler/mailto"] },
    Category { id: "text", name: n_("Text editor"), types: &["text/plain"] },
    Category {
        id: "images",
        name: n_("Images"),
        types: &["image/png", "image/jpeg", "image/gif", "image/webp", "image/svg+xml", "image/bmp", "image/tiff", "image/avif"],
    },
    Category { id: "pdf", name: n_("PDF documents"), types: &["application/pdf"] },
    Category {
        id: "video",
        name: n_("Video"),
        types: &["video/mp4", "video/webm", "video/x-matroska", "video/quicktime", "video/mpeg", "video/ogg"],
    },
    Category {
        id: "music",
        name: n_("Music"),
        types: &["audio/mpeg", "audio/flac", "audio/ogg", "audio/x-wav", "audio/mp4", "audio/aac", "audio/opus"],
    },
];

const SECTION: &str = "[Default Applications]";

fn is_section(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('[') && line.ends_with(']')
}

/// A chave de uma linha `tipo=valor` (sem os espaços em volta), ou None num comentário ou numa linha vazia.
fn key_of(line: &str) -> Option<&str> {
    let line = line.trim_start();
    if line.starts_with('#') {
        return None;
    }
    line.split_once('=').map(|(key, _)| key.trim())
}

/// O app padrão do usuário para um tipo: o primeiro .desktop da linha dele no [Default Applications], ou None.
pub fn user_default(text: &str, mime: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        if is_section(line) {
            inside = line.trim() == SECTION;
            continue;
        }
        if inside && key_of(line) == Some(mime) {
            let value = line.split_once('=').map(|(_, v)| v).unwrap_or_default();
            return value.split(';').map(str::trim).find(|v| !v.is_empty()).map(str::to_string);
        }
    }
    None
}

/// O texto com o [Default Applications] dando `desktop` (um id como "firefox.desktop") a cada um dos `types`, ou
/// tirando-os (`desktop` None). As outras linhas, seções e comentários ficam como estão; a seção é criada no fim
/// se faltar. Uma linha repetida do mesmo tipo some junto.
pub fn set_defaults(text: &str, types: &[&str], desktop: Option<&str>) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    let mut found_section = false;
    // Onde acaba a seção (a próxima linha depois do último item dela), para pôr os tipos novos
    let mut section_end: Option<usize> = None;
    for line in text.lines() {
        if is_section(line) {
            inside = line.trim() == SECTION;
            out.push(line.to_string());
            if inside {
                found_section = true;
                section_end = Some(out.len());
            }
            continue;
        }
        if inside && key_of(line).is_some_and(|key| types.contains(&key)) {
            continue;
        }
        out.push(line.to_string());
        if inside && !line.trim().is_empty() {
            section_end = Some(out.len());
        }
    }
    if let Some(desktop) = desktop {
        let lines: Vec<String> = types.iter().map(|mime| format!("{mime}={desktop};")).collect();
        match section_end {
            Some(at) if found_section => {
                out.splice(at..at, lines);
            }
            _ => {
                if out.last().is_some_and(|l| !l.trim().is_empty()) {
                    out.push(String::new());
                }
                out.push(SECTION.to_string());
                out.extend(lines);
            }
        }
    }
    let mut text = out.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

/// Os tipos da categoria que um app abre (os do MimeType dele): só esses vão para ele, nenhum tipo fica com um
/// app que não o abre.
pub fn types_for<'a>(category: &Category, app_types: &[&str]) -> Vec<&'a str>
where
    'static: 'a,
{
    category.types.iter().copied().filter(|t| app_types.contains(t)).collect()
}

/// Os mimeapps.list das camadas do sistema, na ordem da especificação: em cada pasta do XDG_CONFIG_DIRS e depois em
/// cada `<pasta>/applications` do XDG_DATA_DIRS, o `<desktop>-mimeapps.list` de cada nome do XDG_CURRENT_DESKTOP
/// (em minúsculas) antes do `mimeapps.list`. As do usuário (o ~/.config) ficam de fora: é a escolha dele.
pub fn mimeapps_files(desktops: Option<&str>, config_dirs: &[PathBuf], data_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let names: Vec<String> = desktops
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| format!("{}-mimeapps.list", d.to_lowercase()))
        .chain(std::iter::once("mimeapps.list".to_string()))
        .collect();
    let dirs = config_dirs.iter().cloned().chain(data_dirs.iter().map(|d| d.join("applications")));
    dirs.flat_map(|dir| names.iter().map(move |name| dir.join(name))).collect()
}

/// O padrão do sistema para um tipo: o da primeira camada (os textos, na ordem do [`mimeapps_files`]) que o dá.
pub fn system_default(layers: &[String], mime: &str) -> Option<String> {
    layers.iter().find_map(|text| user_default(text, mime))
}

/// Grava o arquivo inteiro: por um arquivo ao lado, sincronizado, e um rename, para quem lê no meio (o xdg-open, o
/// Gio) nunca ver meio arquivo, nem um vazio depois de uma queda. Num link (um mimeapps.list do stow ou do
/// chezmoi), grava no arquivo para onde ele aponta: o rename por cima do link o trocaria por um arquivo solto.
pub fn write_file(path: &std::path::Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let is_link = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    let path = if is_link {
        // Um link quebrado: recusa, sem trocá-lo por um arquivo solto
        std::fs::canonicalize(path).map_err(|_| format!("{}: points to a missing file", path.display()))?
    } else {
        path.to_path_buf()
    };
    // As permissões de antes (um 0600 do usuário), em vez das do umask
    let mode = std::fs::metadata(&path).ok().map(|m| m.permissions());
    let dir = path.parent().ok_or_else(|| format!("{}: no folder", path.display()))?;
    std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = dir.join(format!(".{name}.swaydesk-settings"));
    let written = std::fs::File::create(&temp)
        .and_then(|mut file| {
            if let Some(mode) = mode {
                file.set_permissions(mode)?;
            }
            file.write_all(text.as_bytes()).and_then(|()| file.sync_all())
        })
        .and_then(|()| std::fs::rename(&temp, &path));
    written.map_err(|err| {
        let _ = std::fs::remove_file(&temp);
        format!("{}: {err}", path.display())
    })
}

/// O nome de um item da lista: o padrão do sistema (com o nome do app que vale, se houver) ou um app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    System(Option<String>),
    App(String),
}

/// Um item: o .desktop do app, ou None no padrão do sistema (tirar a linha do usuário).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: Option<String>,
    pub label: Label,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choices {
    pub items: Vec<Choice>,
    pub selected: usize,
}

/// A lista de uma categoria: o padrão do sistema primeiro (com `system_name`, o app que vale sem a escolha do
/// usuário), depois os candidatos (id, nome), e a escolha do usuário (`user`) marcada. Uma escolha que não é
/// candidata (o app saiu, ou não declara o tipo) entra no fim com o id, para não sumir sem aviso.
pub fn choices(user: Option<&str>, system_name: Option<&str>, candidates: &[(String, String)]) -> Choices {
    let mut items = vec![Choice { id: None, label: Label::System(system_name.map(str::to_string)) }];
    for (id, name) in candidates {
        if !items.iter().any(|i| i.id.as_deref() == Some(id)) {
            items.push(Choice { id: Some(id.clone()), label: Label::App(name.clone()) });
        }
    }
    let selected = match user {
        None => 0,
        Some(user) => match items.iter().position(|i| i.id.as_deref() == Some(user)) {
            Some(at) => at,
            None => {
                items.push(Choice { id: Some(user.to_string()), label: Label::App(user.to_string()) });
                items.len() - 1
            }
        },
    };
    Choices { items, selected }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEB: &[&str] = &["x-scheme-handler/http", "x-scheme-handler/https"];

    #[test]
    fn the_user_default_is_the_first_desktop_of_its_line() {
        let text = "[Added Associations]\ntext/plain=gedit.desktop;\n\n[Default Applications]\n# comment\n\
                    text/plain = org.gnome.TextEditor.desktop;gedit.desktop;\nimage/png=;\n";
        assert_eq!(user_default(text, "text/plain").as_deref(), Some("org.gnome.TextEditor.desktop"));
        // Só o [Default Applications] conta, e uma linha vazia é como nenhuma
        assert_eq!(user_default("[Added Associations]\ntext/plain=gedit.desktop;\n", "text/plain"), None);
        assert_eq!(user_default(text, "image/png"), None);
        assert_eq!(user_default("", "text/plain"), None);
    }

    #[test]
    fn setting_replaces_the_types_and_keeps_everything_else() {
        let text = "# by hand\n[Default Applications]\nx-scheme-handler/http=chromium.desktop;\ntext/plain=gedit.desktop;\n\n\
                    [Added Associations]\nx-scheme-handler/http=chromium.desktop;\n";
        let got = set_defaults(text, WEB, Some("firefox.desktop"));
        assert_eq!(
            got,
            "# by hand\n[Default Applications]\ntext/plain=gedit.desktop;\nx-scheme-handler/http=firefox.desktop;\n\
             x-scheme-handler/https=firefox.desktop;\n\n[Added Associations]\nx-scheme-handler/http=chromium.desktop;\n"
        );
        assert_eq!(user_default(&got, "x-scheme-handler/https").as_deref(), Some("firefox.desktop"));
    }

    #[test]
    fn without_the_file_or_the_section_it_is_made() {
        assert_eq!(set_defaults("", &["application/pdf"], Some("org.gnome.Papers.desktop")),
                   "[Default Applications]\napplication/pdf=org.gnome.Papers.desktop;\n");
        assert_eq!(set_defaults("[Added Associations]\na/b=x.desktop;", &["a/b"], Some("y.desktop")),
                   "[Added Associations]\na/b=x.desktop;\n\n[Default Applications]\na/b=y.desktop;\n");
    }

    #[test]
    fn the_system_default_takes_only_the_users_lines_away() {
        let text = "[Default Applications]\nx-scheme-handler/http=firefox.desktop;\nx-scheme-handler/http=old.desktop;\n\
                    text/plain=gedit.desktop;\n[Added Associations]\nx-scheme-handler/http=firefox.desktop;\n";
        assert_eq!(
            set_defaults(text, WEB, None),
            "[Default Applications]\ntext/plain=gedit.desktop;\n[Added Associations]\nx-scheme-handler/http=firefox.desktop;\n"
        );
        assert_eq!(set_defaults("", WEB, None), "");
    }

    #[test]
    fn an_app_gets_only_the_types_it_opens() {
        let images = CATEGORIES.iter().find(|c| c.id == "images").unwrap();
        assert_eq!(types_for(images, &["image/png", "image/jpeg", "text/plain"]), ["image/png", "image/jpeg"]);
        assert!(types_for(images, &["text/plain"]).is_empty());
    }

    fn c(id: &str, name: &str) -> (String, String) {
        (id.to_string(), name.to_string())
    }

    #[test]
    fn the_list_starts_with_the_system_default_and_selects_the_users() {
        let candidates = [c("firefox.desktop", "Firefox"), c("chromium.desktop", "Chromium")];
        // Sem escolha do usuário: o padrão do sistema, com o nome do app que vale
        let list = choices(None, Some("Firefox"), &candidates);
        assert_eq!(list.items[0], Choice { id: None, label: Label::System(Some("Firefox".into())) });
        assert_eq!(list.items[1..].iter().map(|i| i.id.as_deref().unwrap()).collect::<Vec<_>>(), ["firefox.desktop", "chromium.desktop"]);
        assert_eq!(list.selected, 0);
        // A escolha do usuário fica marcada
        assert_eq!(choices(Some("chromium.desktop"), None, &candidates).selected, 2);
        // Sem nenhum app no sistema
        assert_eq!(choices(None, None, &[]).items, [Choice { id: None, label: Label::System(None) }]);
    }

    #[test]
    fn a_user_choice_that_is_not_a_candidate_still_shows() {
        let list = choices(Some("gone.desktop"), None, &[c("firefox.desktop", "Firefox")]);
        assert_eq!(list.items.last(), Some(&Choice { id: Some("gone.desktop".into()), label: Label::App("gone.desktop".into()) }));
        assert_eq!(list.selected, 2);
    }

    #[test]
    fn the_system_default_is_the_first_layer_that_names_one() {
        let layers = [
            String::from("[Default Applications]\ntext/plain=gedit.desktop;\n"),
            String::from("[Default Applications]\nimage/png=org.gnome.Loupe.desktop;\n"),
            String::from("[Default Applications]\nimage/png=eog.desktop;\napplication/pdf=org.gnome.Papers.desktop;\n"),
        ];
        assert_eq!(system_default(&layers, "image/png").as_deref(), Some("org.gnome.Loupe.desktop"));
        assert_eq!(system_default(&layers, "application/pdf").as_deref(), Some("org.gnome.Papers.desktop"));
        assert_eq!(system_default(&layers, "video/mp4"), None);
    }

    #[test]
    fn mimeapps_files_in_the_xdg_order() {
        let got = mimeapps_files(Some("sway"), &["/etc/xdg".into(), "/opt/xdg".into()], &["/usr/share".into()]);
        let got: Vec<&str> = got.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(got, [
            "/etc/xdg/sway-mimeapps.list", "/etc/xdg/mimeapps.list",
            "/opt/xdg/sway-mimeapps.list", "/opt/xdg/mimeapps.list",
            "/usr/share/applications/sway-mimeapps.list", "/usr/share/applications/mimeapps.list",
        ]);
        // XDG_CURRENT_DESKTOP pode ter vários nomes; sem ele, só os mimeapps.list
        let got = mimeapps_files(Some("Sway:GNOME"), &["/etc/xdg".into()], &[]);
        assert_eq!(got.iter().map(|p| p.to_str().unwrap()).collect::<Vec<_>>(),
                   ["/etc/xdg/sway-mimeapps.list", "/etc/xdg/gnome-mimeapps.list", "/etc/xdg/mimeapps.list"]);
        assert_eq!(mimeapps_files(None, &["/etc/xdg".into()], &[]).len(), 1);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swaydesk-apps-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writing_replaces_the_file_whole() {
        let dir = temp_dir("plain");
        let path = dir.join("config/mimeapps.list");
        write_file(&path, "[Default Applications]\na/b=x.desktop;\n").unwrap();
        write_file(&path, "[Default Applications]\na/b=y.desktop;\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[Default Applications]\na/b=y.desktop;\n");
        // Nada sobra ao lado
        assert_eq!(std::fs::read_dir(dir.join("config")).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_linked_file_keeps_its_link_and_the_target_changes() {
        // Um mimeapps.list do stow ou do chezmoi: o link fica, e o arquivo do repositório dos dotfiles muda
        let dir = temp_dir("link");
        std::fs::create_dir_all(dir.join("dotfiles")).unwrap();
        std::fs::create_dir_all(dir.join("config")).unwrap();
        let target = dir.join("dotfiles/mimeapps.list");
        std::fs::write(&target, "old\n").unwrap();
        let link = dir.join("config/mimeapps.list");
        std::os::unix::fs::symlink("../dotfiles/mimeapps.list", &link).unwrap();
        write_file(&link, "new\n").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_files_mode_is_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("mode");
        let path = dir.join("mimeapps.list");
        std::fs::write(&path, "old\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_file(&path, "new\n").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_link_to_a_missing_file_is_refused_and_says_so() {
        let dir = temp_dir("dangling");
        let link = dir.join("mimeapps.list");
        std::os::unix::fs::symlink("gone/mimeapps.list", &link).unwrap();
        let err = write_file(&link, "new\n").unwrap_err();
        assert!(err.contains("points to a missing file"), "{err}");
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_categories_and_their_first_type() {
        let ids: Vec<&str> = CATEGORIES.iter().map(|c| c.id).collect();
        assert_eq!(ids, ["web", "mail", "text", "images", "pdf", "video", "music"]);
        let first: Vec<&str> = CATEGORIES.iter().map(|c| c.types[0]).collect();
        assert_eq!(first, ["x-scheme-handler/http", "x-scheme-handler/mailto", "text/plain", "image/png", "application/pdf", "video/mp4", "audio/mpeg"]);
    }
}
