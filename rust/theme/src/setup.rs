//! O primeiro login na sessão do pacote swaydesk-desktop: os arquivos de cada usuário (o `home-copy/` do
//! repositório, que o pacote instala como `skel`) vão para o `~`, com as mesmas regras do `install.sh`:
//! - só o que falta: um arquivo que já existe (mesmo um link quebrado) é do usuário e fica;
//! - o `50-local.conf` sai do `local_conf::template`, na língua da sessão (o do skel é o mesmo em português), e
//!   ganha o teclado do sistema (`/etc/default/keyboard`), como o `keyboard_block`; sem esse
//!   arquivo (Arch), o do systemd: o layout X11 do localectl, senão o mapa do console traduzido pelo
//!   kbd-model-map. A cada login, o bloco segue o teclado do sistema (localectl, dpkg-reconfigure), enquanto
//!   for o que gravamos: um bloco mexido pelo usuário fica;
//! - com a sessão no US internacional (o teclado que o Sway acaba usando, pelo `config.d`), o `~/.XCompose`
//!   com ' + c dando ç, se não houver nada lá; ele sai quando a sessão muda, se ainda for igual ao nosso;
//! - os fixados da dock trocam o Nemo do modelo pelo gerenciador de arquivos da config;
//! - quem já tinha os fixados com o Nemo do modelo sem o Nemo instalado (a 0.1.0 e a 0.1.1 na ISO, onde o
//!   swaydesk-pin-installer copiava o modelo cru) ganha a mesma troca, uma vez: depois dela não há mais "nemo".
use std::path::{Path, PathBuf};

use swaydesk_common::config;
use swaydesk_common::keyboard as kbd;

use crate::{desktop, local_conf};

const LOCAL_CONF: &str = ".config/sway/config.d/50-local.conf";
const PINNED: &str = ".local/share/sway-dock/pinned";
/// No skel, com o nome velho; no home, vai para a pasta de config que vale (`target`)
const CONFIG: &str = ".config/sway-desktop/config.toml";
const XCOMPOSE: &str = ".XCompose";
/// O último bloco do teclado que gravamos no `50-local.conf` (vazio: nenhum). Igual ao do arquivo: ninguém o
/// mexeu, e ele pode seguir o teclado do sistema.
const KEYBOARD_STAMP: &str = ".local/state/swaydesk/keyboard-block";

/// O skel do pacote: SWAYDESK_SKEL, senão o swaydesk-desktop/skel ao lado do share/sway-desktop da instalação
/// (/usr/share/swaydesk-desktop/skel, para o binário em /usr/bin).
pub fn skel_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SWAYDESK_SKEL").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let share = desktop::share_dir();
    share.parent().map(|p| p.join("swaydesk-desktop/skel")).unwrap_or_else(|| share.join("../swaydesk-desktop/skel"))
}

/// Os arquivos do skel, relativos a ele e em ordem. O 50-local.conf do skel só marca que o arquivo vem: o
/// conteúdo sai do `local_conf::template`, na língua da sessão (a config.toml vem antes dos fixados da dock). Só arquivos
/// comuns: um link no skel (o home-copy/ não tem nenhum) ficaria de fora.
fn skel_files(skel: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![skel.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(kind) if kind.is_file() => {
                    if let Ok(rel) = path.strip_prefix(skel) {
                        files.push(rel.to_path_buf());
                    }
                }
                _ => {}
            }
        }
    }
    files.sort();
    files
}

/// Troca a linha "nemo" pelo .desktop do gerenciador da config, se for um nome seguro.
fn pinned_with(text: &str, file_manager: &str) -> String {
    if file_manager.is_empty() || !file_manager.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)) {
        return text.to_string();
    }
    text.lines().map(|line| if line == "nemo" { file_manager } else { line }).map(|line| format!("{line}\n")).collect()
}

/// A migração dos fixados de quem já tinha o arquivo: a linha "nemo" com o Nemo fora do sistema vira o
/// gerenciador resolvido (ou some, se ele já estiver fixado). None: nada a mudar. Um "nemo" com o Nemo
/// instalado é uma escolha do usuário e fica; um gerenciador resolvido que também é o Nemo, ou um nome
/// estranho, não muda nada.
fn migrated_pins(text: &str, nemo_installed: bool, file_manager: &str) -> Option<String> {
    if nemo_installed || file_manager == "nemo" || !text.lines().any(|l| l.trim() == "nemo") {
        return None;
    }
    if file_manager.is_empty() || !file_manager.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)) {
        return None;
    }
    let already = text.lines().any(|l| l.trim() == file_manager);
    let mut swapped = already;
    let lines = text.lines().filter_map(|line| {
        if line.trim() != "nemo" {
            return Some(line);
        }
        if swapped {
            return None; // o gerenciador já está fixado, ou o Nemo aparecia duas vezes
        }
        swapped = true;
        Some(file_manager)
    });
    Some(lines.map(|line| format!("{line}\n")).collect())
}

/// A config.toml do skel com um tema GTK que exista: o padrão é o Mint-Y-Dark-Teal, que só o Mint tem. No
/// Debian, no Ubuntu e no Arch, fica o tema de reserva (o adw-gtk3-dark), em vez de o app Tema avisar que o
/// tema da config não está instalado. Com o snapd-desktop-integration (snaps), o Adwaita-dark (desktop::fallback_with).
/// No Mint, nada muda.
fn with_installed_theme(text: &str, installed: &dyn Fn(&str) -> bool, snaps: bool) -> String {
    let fallback = desktop::fallback_with(installed, snaps);
    text.lines()
        .map(|line| match line.strip_prefix("gtk_theme = \"").and_then(|rest| rest.strip_suffix('"')) {
            Some(theme) if !installed(theme) => format!("gtk_theme = \"{fallback}\"\n"),
            _ => format!("{line}\n"),
        })
        .collect()
}

/// Onde um arquivo do skel vai no home: o que está em .config/sway-desktop vai para a pasta de config que vale
/// (a nova, ou a velha enquanto ela é uma pasta de verdade ou um link do usuário).
fn target(home: &Path, rel: &Path) -> PathBuf {
    let config = home.join(".config");
    match rel.strip_prefix(".config/sway-desktop") {
        Ok(rest) => swaydesk_common::dirs::pick(&config.join(swaydesk_common::dirs::OLD), &config.join(swaydesk_common::dirs::NEW)).join(rest),
        Err(_) => home.join(rel),
    }
}

/// Copia do skel para o home o que falta. Devolve os caminhos copiados, relativos ao home.
pub fn setup(skel: &Path, home: &Path, keyboard: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut copied = Vec::new();
    for rel in skel_files(skel) {
        let target = target(home, &rel);
        // symlink_metadata: um link quebrado também é do usuário
        if let Ok(meta) = target.symlink_metadata() {
            // Os fixados de antes: só um arquivo comum (um link é do usuário)
            if rel == Path::new(PINNED) && meta.file_type().is_file() {
                migrate_pins(&target)?;
            }
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if rel == Path::new(LOCAL_CONF) {
            // O modelo na língua da sessão; o do skel é o mesmo em português (o do install.sh)
            std::fs::write(&target, local_conf::template(&|msgid| swaydesk_common::i18n::tr(msgid)))?;
        } else {
            std::fs::copy(skel.join(&rel), &target)?;
        }
        if rel == Path::new(CONFIG) {
            let text = std::fs::read_to_string(&target)?;
            std::fs::write(&target, with_installed_theme(&text, &desktop::theme_installed, desktop::snap_theme_prompt()))?;
        } else if rel == Path::new(PINNED) {
            // A config.toml, se veio do skel, já foi copiada: vem antes na ordem. O config::load lê a do HOME
            // do ambiente, que no CLI é este home
            let manager = desktop::get(&config::load(), "file_manager_desktop").unwrap_or_default();
            let text = std::fs::read_to_string(&target)?;
            std::fs::write(&target, pinned_with(&text, &manager))?;
        }
        copied.push(rel);
    }
    // O bloco do teclado do sistema (no 50-local.conf recém-copiado, sem carimbo nem bloco, ele entra no fim e
    // ganha o carimbo), o que mudou nele e o ç no US internacional: não impedem o resto do login
    if let Err(err) = sync_keyboard(home, keyboard, &|msgid| swaydesk_common::i18n::tr(msgid)) {
        eprintln!("swaydesk-desktop-config: {}: {err}", home.join(LOCAL_CONF).display());
    }
    let intl = kbd::is_us_intl(&kbd::session_keyboard(&sway_confs(home)));
    if let Err(err) = sync_compose(home, intl) {
        eprintln!("swaydesk-desktop-config: {}: {err}", home.join(XCOMPOSE).display());
    }
    Ok(copied)
}

/// O bloco do teclado que a sessão grava no `50-local.conf`, com o título na língua da sessão (numa linha só:
/// uma quebra na tradução deixaria uma linha sem o #, que o Sway leria como config).
fn login_block(file: &Path, text: &str, tr: &dyn Fn(&str) -> String) -> String {
    // TRANSLATORS: the title of the system keyboard block in ~/.config/sway/config.d/50-local.conf; %s is the file it comes from
    let title = tr("System keyboard (%s): it follows the system's at each login until you edit it");
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ").replacen("%s", &file.display().to_string(), 1);
    kbd::keyboard_block_titled(&title, text)
}

/// Se o texto tem um `input type:keyboard` ou `input *` fora de comentário, com qualquer cabeçalho.
fn has_keyboard_input(text: &str) -> bool {
    text.lines().any(|line| {
        let words: Vec<&str> = line.split('#').next().unwrap_or("").split_whitespace().map(|w| w.trim_matches('"')).collect();
        matches!(words.as_slice(), ["input", "type:keyboard" | "*", ..])
    })
}

/// O bloco do teclado no `50-local.conf` segue o do sistema, se ainda for o que gravamos: o texto do carimbo,
/// achado no arquivo (em qualquer língua, e com o cabeçalho de antes da 0.1.8, o "lido na instalação"), ou, sem
/// carimbo (a conta de antes dele), nenhum bloco ou um bloco intocado com aquele cabeçalho. Um bloco mexido, ou
/// tirado depois do carimbo, é do usuário e fica, e o carimbo também. Só num arquivo comum (um link é do usuário).
fn sync_keyboard(home: &Path, keyboard: &Path, tr: &dyn Fn(&str) -> String) -> std::io::Result<()> {
    let path = home.join(LOCAL_CONF);
    if !path.symlink_metadata().is_ok_and(|m| m.file_type().is_file()) {
        return Ok(());
    }
    let text = std::fs::read_to_string(&path)?;
    let stamp_path = home.join(KEYBOARD_STAMP);
    let (range, ours) = match std::fs::read_to_string(&stamp_path) {
        // Carimbo vazio: não gravamos bloco nenhum. O nosso vai para o fim só se não houver outro bloco de teclado
        // (o do install.sh, um do usuário): depois dele, o nosso valeria mais no Sway
        Ok(stamp) if stamp.is_empty() => (None, !has_keyboard_input(&text)),
        Ok(stamp) => match text.find(&stamp) {
            Some(start) => (Some(start..start + stamp.len()), true),
            None => (None, false),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let range = kbd::find_block(&text);
            let current = range.clone().map(|r| &text[r]).unwrap_or("");
            let ours = current.is_empty() || kbd::is_pristine_block(current);
            (range, ours)
        }
        Err(err) => return Err(err),
    };
    if !ours {
        return Ok(());
    }
    let current = range.clone().map(|r| &text[r]).unwrap_or("");
    let new = kbd::system_keyboard(keyboard).map(|(source, text)| login_block(&source, &text, tr)).unwrap_or_default();
    if new != current {
        let updated = match range {
            Some(r) => format!("{}{new}{}", &text[..r.start], &text[r.end..]),
            None => text.clone() + &new,
        };
        let mode = std::fs::metadata(&path).map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o7777).unwrap_or(0o644);
        swaydesk_common::text::atomic_write(&path, updated.as_bytes(), mode)?;
    }
    if std::fs::read_to_string(&stamp_path).ok().as_deref() != Some(new.as_str()) {
        swaydesk_common::text::atomic_write(&stamp_path, new.as_bytes(), 0o644)?;
    }
    Ok(())
}

/// Os arquivos do `~/.config/sway/config.d`, na ordem do `include` (a do nome), para saber o teclado da sessão.
fn sway_confs(home: &Path) -> Vec<String> {
    let dir = home.join(".config/sway/config.d");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir).map(|e| e.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    names.sort();
    names.iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect()
}

/// O `~/.XCompose` do US internacional: os apps GTK, Qt e os terminais usam a tabela do locale, que no en_US
/// dá ć para ' + c. Com a sessão no US internacional, cria o nosso se não houver nada no caminho (nem um
/// link, nem quebrado: create_new não segue nem troca). Fora dele, apaga só o que ainda é igual ao nosso: um
/// arquivo mexido pelo usuário fica.
fn sync_compose(home: &Path, intl: bool) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = home.join(XCOMPOSE);
    let meta = path.symlink_metadata();
    if intl {
        if meta.is_ok() {
            return Ok(());
        }
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).open(&path)?;
        return file.write_all(kbd::INTL_COMPOSE.as_bytes());
    }
    if meta.is_ok_and(|m| m.file_type().is_file())
        && std::fs::read(&path).is_ok_and(|data| data == kbd::INTL_COMPOSE.as_bytes())
    {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Aplica a migração no arquivo de fixados do usuário, se houver o que mudar.
fn migrate_pins(path: &Path) -> std::io::Result<()> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(()) }; // ilegível: a dock cuida dele
    let manager = desktop::get(&config::load(), "file_manager_desktop").unwrap_or_default();
    if let Some(new) = migrated_pins(&text, crate::which("nemo"), &manager) {
        swaydesk_common::text::atomic_write(path, new.as_bytes(), 0o600)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swaydesk-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_intl_compose_table_comes_and_goes_with_the_keyboard() {
        let home = tempdir("compose");
        let path = home.join(XCOMPOSE);
        sync_compose(&home, false).unwrap();
        assert!(path.symlink_metadata().is_err());
        sync_compose(&home, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), kbd::INTL_COMPOSE);
        sync_compose(&home, true).unwrap();
        sync_compose(&home, false).unwrap();
        assert!(path.symlink_metadata().is_err());
        // O do usuário fica nos dois casos: um arquivo dele, um mexido, um link (mesmo quebrado)
        std::fs::write(&path, "include \"%L\"\n").unwrap();
        sync_compose(&home, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "include \"%L\"\n");
        std::fs::write(&path, format!("{}<Multi_key> <a> : \"á\"\n", kbd::INTL_COMPOSE)).unwrap();
        sync_compose(&home, false).unwrap();
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(home.join("nao-existe"), &path).unwrap();
        sync_compose(&home, true).unwrap();
        sync_compose(&home, false).unwrap();
        assert!(path.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(home.join("nao-existe").symlink_metadata().is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    /// O catálogo dos testes: a língua da sessão sem traduzir (o msgid em inglês)
    fn english(msgid: &str) -> String {
        msgid.to_string()
    }

    /// Uma língua de mentira, para ver o cabeçalho traduzido
    fn other_language(msgid: &str) -> String {
        msgid.replace("System keyboard", "Systemtastatur")
    }

    #[test]
    fn the_block_header_is_in_the_session_language_and_says_when_it_changes() {
        let dir = tempdir("header");
        let (home, keyboard) = (dir.join("home"), dir.join("keyboard"));
        let conf = home.join(LOCAL_CONF);
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        std::fs::write(&keyboard, "XKBLAYOUT=\"br\"\nXKBVARIANT=\"abnt2\"\n").unwrap();
        std::fs::write(&conf, "# --- Mouse ---\n").unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        let text = std::fs::read_to_string(&conf).unwrap();
        // Não diz mais "lido na instalação": o pacote grava o bloco no primeiro login e o atualiza a cada login
        assert!(text.contains(&format!(
            "\n# --- System keyboard ({}): it follows the system's at each login until you edit it ---\ninput type:keyboard {{\n",
            keyboard.display()
        )), "{text}");
        assert!(!text.contains("instalação"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_block_is_found_by_the_stamp_in_any_language() {
        // Uma conta que trocou de língua: o bloco gravado em outra continua nosso pelo carimbo, e segue o sistema
        let dir = tempdir("language");
        let (home, keyboard) = (dir.join("home"), dir.join("keyboard"));
        let conf = home.join(LOCAL_CONF);
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        let intl = "XKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n";
        let br = "XKBLAYOUT=\"br\"\nXKBVARIANT=\"abnt2\"\n";
        let head = "# --- Mouse ---\n";
        let before = login_block(&keyboard, intl, &other_language);
        std::fs::write(&conf, format!("{head}{before}# depois do bloco\n")).unwrap();
        std::fs::create_dir_all(home.join(KEYBOARD_STAMP).parent().unwrap()).unwrap();
        std::fs::write(home.join(KEYBOARD_STAMP), &before).unwrap();
        std::fs::write(&keyboard, br).unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        let after = login_block(&keyboard, br, &english);
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), format!("{head}{after}# depois do bloco\n"));
        assert_eq!(std::fs::read_to_string(home.join(KEYBOARD_STAMP)).unwrap(), after);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_install_header_block_with_its_stamp_moves_to_the_new_header() {
        // As contas de agora (0.1.3.1 em diante): o bloco com o cabeçalho velho e o carimbo igual a ele
        let dir = tempdir("old-header");
        let (home, keyboard) = (dir.join("home"), dir.join("keyboard"));
        let conf = home.join(LOCAL_CONF);
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        let br = "XKBLAYOUT=\"br\"\nXKBVARIANT=\"abnt2\"\n";
        let old = kbd::keyboard_block_text(&keyboard, br);
        std::fs::write(&conf, format!("# --- Mouse ---\n{old}")).unwrap();
        std::fs::create_dir_all(home.join(KEYBOARD_STAMP).parent().unwrap()).unwrap();
        std::fs::write(home.join(KEYBOARD_STAMP), &old).unwrap();
        std::fs::write(&keyboard, br).unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        let new = login_block(&keyboard, br, &english);
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), format!("# --- Mouse ---\n{new}"));
        assert_eq!(std::fs::read_to_string(home.join(KEYBOARD_STAMP)).unwrap(), new);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_an_empty_stamp_a_keyboard_block_from_elsewhere_stays_alone() {
        // Carimbo vazio: no último login o sistema não tinha teclado e não gravamos bloco. Um bloco que apareceu
        // depois (o do install.sh, com o cabeçalho velho, ou um do usuário) é dele: o nosso não entra depois
        // dele, onde valeria mais no Sway
        let dir = tempdir("empty-stamp");
        let (home, keyboard) = (dir.join("home"), dir.join("keyboard"));
        let conf = home.join(LOCAL_CONF);
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        std::fs::create_dir_all(home.join(KEYBOARD_STAMP).parent().unwrap()).unwrap();
        std::fs::write(&keyboard, "XKBLAYOUT=\"br\"\nXKBVARIANT=\"abnt2\"\n").unwrap();
        let old = format!("# --- Mouse ---\n{}", kbd::keyboard_block_text(&keyboard, "XKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n"));
        let custom = "# --- Mouse ---\n\n# meu teclado\ninput type:keyboard {\n    xkb_layout de\n}\n";
        for text in [old.as_str(), custom] {
            std::fs::write(&conf, text).unwrap();
            std::fs::write(home.join(KEYBOARD_STAMP), "").unwrap();
            sync_keyboard(&home, &keyboard, &english).unwrap();
            assert_eq!(std::fs::read_to_string(&conf).unwrap(), text);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_keyboard_block_follows_the_system_while_it_is_ours() {
        let dir = tempdir("follow");
        let (home, keyboard) = (dir.join("home"), dir.join("keyboard"));
        let conf = home.join(LOCAL_CONF);
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        let intl = "XKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n";
        let br = "XKBLAYOUT=\"br\"\nXKBVARIANT=\"abnt2\"\n";
        let block = |text: &str| login_block(&keyboard, text, &english);
        let session = |home: &Path| kbd::session_keyboard(&sway_confs(home));
        let head = "# --- Mouse ---\n# input type:pointer {\n# }\n";
        // A conta de antes do carimbo, com o bloco intocado (o cabeçalho velho, o do install.sh): segue o sistema,
        // com o cabeçalho novo, e ganha o carimbo
        std::fs::write(&conf, format!("{head}{}", kbd::keyboard_block_text(&keyboard, intl))).unwrap();
        std::fs::write(&keyboard, br).unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), format!("{head}{}", block(br)));
        assert_eq!(std::fs::read_to_string(home.join(KEYBOARD_STAMP)).unwrap(), block(br));
        assert_eq!(session(&home), "XKBLAYOUT=\"br\"\nXKBVARIANT=\"abnt2\"\n");
        // De volta ao us intl, e depois ao us sem nada: o bloco sai
        std::fs::write(&keyboard, intl).unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        assert!(kbd::is_us_intl(&session(&home)));
        std::fs::write(&keyboard, "XKBLAYOUT=\"us\"\n").unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), head);
        assert_eq!(std::fs::read_to_string(home.join(KEYBOARD_STAMP)).unwrap(), "");
        // Sem bloco e com o carimbo vazio: volta quando o sistema tem um teclado, no fim do arquivo
        std::fs::write(&keyboard, intl).unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), format!("{head}{}", block(intl)));
        // Um bloco mexido fica, e o carimbo também
        let mine = format!("{head}{}", block(intl).replace("}\n", "    xkb_options caps:escape\n}\n"));
        std::fs::write(&conf, &mine).unwrap();
        std::fs::write(&keyboard, br).unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), mine);
        assert_eq!(std::fs::read_to_string(home.join(KEYBOARD_STAMP)).unwrap(), block(intl));
        assert!(kbd::is_us_intl(&session(&home)), "a sessão continua no intl, o ç fica");
        // Tirado pelo usuário depois do carimbo: não volta
        std::fs::write(&conf, head).unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), head);
        // Sem carimbo e com um bloco mexido (a conta de antes dele, que só tem o cabeçalho velho): fica
        std::fs::remove_file(home.join(KEYBOARD_STAMP)).unwrap();
        let old_mine = format!("{head}{}", kbd::keyboard_block_text(&keyboard, intl).replace("}\n", "    xkb_options caps:escape\n}\n"));
        std::fs::write(&conf, &old_mine).unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), old_mine);
        assert!(home.join(KEYBOARD_STAMP).symlink_metadata().is_err());
        // Um link no lugar do 50-local.conf é do usuário
        std::fs::remove_file(&conf).unwrap();
        std::fs::write(dir.join("real.conf"), block(intl)).unwrap();
        std::os::unix::fs::symlink(dir.join("real.conf"), &conf).unwrap();
        sync_keyboard(&home, &keyboard, &english).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("real.conf")).unwrap(), block(intl));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pinned_takes_the_file_manager() {
        assert_eq!(pinned_with("firefox\nnemo\nkitty\n", "thunar"), "firefox\nthunar\nkitty\n");
        assert_eq!(pinned_with("nemo\n", "a b"), "nemo\n");
        assert_eq!(pinned_with("nemo\n", ""), "nemo\n");
    }

    #[test]
    fn old_pins_with_nemo_not_installed_get_the_file_manager() {
        // A 0.1.x na ISO: o modelo cru, sem o Nemo
        assert_eq!(migrated_pins("nemo\nkitty\nswaydesk-install\n", false, "thunar").as_deref(),
                   Some("thunar\nkitty\nswaydesk-install\n"));
        // Já com o gerenciador fixado: o "nemo" só some
        assert_eq!(migrated_pins("thunar\nnemo\nkitty\n", false, "thunar").as_deref(), Some("thunar\nkitty\n"));
        // O Nemo instalado: a escolha fica
        assert_eq!(migrated_pins("nemo\nkitty\n", true, "thunar"), None);
        // Sem a linha (o usuário a tirou), ou já migrado: nada
        assert_eq!(migrated_pins("kitty\n", false, "thunar"), None);
        assert_eq!(migrated_pins("thunar\nkitty\n", false, "thunar"), None);
        // A config pede o Nemo, ou um nome estranho: nada
        assert_eq!(migrated_pins("nemo\n", false, "nemo"), None);
        assert_eq!(migrated_pins("nemo\n", false, "a b"), None);
        assert_eq!(migrated_pins("nemo\n", false, ""), None);
    }

    #[test]
    fn a_missing_gtk_theme_becomes_the_fallback() {
        let text = "# o tema\ngtk_theme = \"Mint-Y-Dark-Teal\"\nicon_theme = \"Papirus-Dark\"\n";
        assert_eq!(with_installed_theme(text, &|_| true, false), text);
        assert_eq!(with_installed_theme(text, &|_| true, true), text);
        // Sem o Mint-Y: o adw-gtk3-dark; sem ele também (os dotfiles, sem o pacote), o Adwaita-dark
        assert_eq!(with_installed_theme(text, &|t| t == "adw-gtk3-dark", false),
                   "# o tema\ngtk_theme = \"adw-gtk3-dark\"\nicon_theme = \"Papirus-Dark\"\n");
        assert_eq!(with_installed_theme(text, &|_| false, false),
                   "# o tema\ngtk_theme = \"Adwaita-dark\"\nicon_theme = \"Papirus-Dark\"\n");
        // Com o snapd-desktop-integration, o Adwaita-dark mesmo com o adw-gtk3-dark instalado: os snaps o têm
        assert_eq!(with_installed_theme(text, &|t| t != "Mint-Y-Dark-Teal", true),
                   "# o tema\ngtk_theme = \"Adwaita-dark\"\nicon_theme = \"Papirus-Dark\"\n");
    }

    #[test]
    fn copies_only_what_is_missing() {
        let dir = tempdir("copy");
        let (skel, home) = (dir.join("skel"), dir.join("home"));
        for (rel, text) in [(".config/a/one", "1\n"), (".config/a/two", "2\n"), (".local/b/three", "3\n")] {
            std::fs::create_dir_all(skel.join(rel).parent().unwrap()).unwrap();
            std::fs::write(skel.join(rel), text).unwrap();
        }
        std::fs::create_dir_all(home.join(".config/a")).unwrap();
        std::fs::write(home.join(".config/a/one"), "meu\n").unwrap();
        std::os::unix::fs::symlink(dir.join("nao-existe"), home.join(".config/a/two")).unwrap();
        let copied = setup(&skel, &home, &dir.join("sem-teclado")).unwrap();
        assert_eq!(copied, vec![PathBuf::from(".local/b/three")]);
        assert_eq!(std::fs::read_to_string(home.join(".config/a/one")).unwrap(), "meu\n");
        assert!(home.join(".config/a/two").symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(home.join(".local/b/three")).unwrap(), "3\n");
        // De novo: nada a copiar
        assert!(setup(&skel, &home, &dir.join("sem-teclado")).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_account_gets_one_keyboard_block() {
        // O primeiro login: o modelo do 50-local.conf mais um bloco só, e o carimbo igual a ele (a VM achou dois
        // blocos: a cópia gravava um e o sync_keyboard, sem carimbo, não o reconhecia e gravava outro)
        let dir = tempdir("new-account");
        let (skel, home, keyboard) = (dir.join("skel"), dir.join("home"), dir.join("keyboard"));
        std::fs::create_dir_all(skel.join(LOCAL_CONF).parent().unwrap()).unwrap();
        std::fs::write(skel.join(LOCAL_CONF), "# modelo\n").unwrap();
        std::fs::write(&keyboard, "XKBLAYOUT=\"br\"\nXKBVARIANT=\"abnt2\"\n").unwrap();
        setup(&skel, &home, &keyboard).unwrap();
        let text = std::fs::read_to_string(home.join(LOCAL_CONF)).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("input type:keyboard")).count(), 1, "{text}");
        let stamp = std::fs::read_to_string(home.join(KEYBOARD_STAMP)).unwrap();
        assert!(text.ends_with(&stamp) && !stamp.is_empty(), "{text}\n--- carimbo:\n{stamp}");
        // O segundo login não muda nada
        setup(&skel, &home, &keyboard).unwrap();
        assert_eq!(std::fs::read_to_string(home.join(LOCAL_CONF)).unwrap(), text);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_desktop_config_goes_to_the_folder_in_use() {
        let dir = tempdir("config-dir");
        let home = dir.join("home");
        // Conta nova: a pasta nova
        assert_eq!(target(&home, Path::new(CONFIG)), home.join(".config/swaydesk/config.toml"));
        // A velha de verdade (antes da migração, ou com o rename que falhou) vale
        std::fs::create_dir_all(home.join(".config/sway-desktop")).unwrap();
        assert_eq!(target(&home, Path::new(CONFIG)), home.join(".config/sway-desktop/config.toml"));
        // Migrada: a nova, não pelo link
        std::fs::remove_dir(home.join(".config/sway-desktop")).unwrap();
        std::fs::create_dir_all(home.join(".config/swaydesk")).unwrap();
        std::os::unix::fs::symlink("swaydesk", home.join(".config/sway-desktop")).unwrap();
        assert_eq!(target(&home, Path::new(CONFIG)), home.join(".config/swaydesk/config.toml"));
        assert_eq!(target(&home, Path::new(PINNED)), home.join(PINNED));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
