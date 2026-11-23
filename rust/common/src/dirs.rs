//! As pastas do usuário com o nome novo (`swaydesk`) e a migração das velhas (`sway-desktop`): o porte de
//! `desktop_config.migrate_dirs` e dos helpers `config_dir`/`data_dir`/`state_dir` do Python, com os mesmos casos.
//!
//! Regra de leitura e escrita (todos usam os helpers, quem lê e quem grava): vale a pasta velha enquanto ela é uma
//! pasta de verdade, ou um link que não é o nosso (stow, dotfiles do usuário); senão, a nova. Assim tudo funciona
//! antes da migração (depois de atualizar os pacotes com a sessão aberta), sem ela (o `rename` falhou) e depois de
//! voltar para a 0.1.3.x (a versão velha lê o link, que leva à nova).
//!
//! A migração só roda no login, antes do Sway (`setup` e `sway-session`), e no `install.sh`: nunca no meio da
//! sessão. Por par (velho, novo):
//! 1. o velho é link ou não existe: nada;
//! 2. o novo não existe: `rename(velho, novo)` e, logo depois, o link relativo `velho → novo`;
//! 3. os dois são pastas: se nenhum item do velho existe no novo, os itens vão um a um para o novo, e o velho vira o
//!    link quando ficar vazio; com algum item nos dois, nada muda (vale o velho) e sai um aviso;
//! 4. o `rename` falha: o velho fica e continua valendo. Nunca copiar e apagar.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// O nome novo das pastas, e o velho.
pub const NEW: &str = "swaydesk";
pub const OLD: &str = "sway-desktop";

fn env_dir(name: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(name) {
        // Um XDG_* relativo é ignorado, como pede a especificação: os caminhos e os links dependeriam do cwd
        Some(value) if Path::new(&value).is_absolute() => PathBuf::from(value),
        _ => crate::config::home().join(fallback),
    }
}

/// As bases do XDG que os pares usam, para os testes trocarem sem mexer no ambiente.
#[derive(Clone, Debug)]
pub struct Bases {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

impl Bases {
    pub fn from_env() -> Self {
        Bases {
            config: env_dir("XDG_CONFIG_HOME", ".config"),
            data: env_dir("XDG_DATA_HOME", ".local/share"),
            state: env_dir("XDG_STATE_HOME", ".local/state"),
            cache: env_dir("XDG_CACHE_HOME", ".cache"),
        }
    }

    /// Os pares na ordem da migração (os índices em CONFIG_PAIR e WAYBAR_PAIR): as pastas-mãe antes da
    /// aninhada. Ao contrário, a da Waybar criaria `~/.config/swaydesk` antes, e a config do usuário cairia no
    /// caso 3.
    pub fn pairs(&self) -> Vec<(PathBuf, PathBuf)> {
        vec![
            (self.config.join(OLD), self.config.join(NEW)),
            (self.data.join(OLD), self.data.join(NEW)),
            (self.state.join(OLD), self.state.join(NEW)),
            (self.config.join("swaydesk-desktop/waybar"), self.config.join(NEW).join("waybar")),
        ]
    }

    /// A trava da migração: sempre o mesmo caminho, fora de todos os pares (o cache não migra).
    pub fn lock_file(&self) -> PathBuf {
        self.cache.join("swaydesk-migrate.lock")
    }
}

const CONFIG_PAIR: usize = 0;
const WAYBAR_PAIR: usize = 3;

/// O alvo do nosso link no lugar de `old`: o caminho de `new` relativo à pasta de `old` (o nome, quando os dois
/// são irmãos). Relativo, o link sobrevive a uma HOME movida e a um backup restaurado.
pub fn link_target(old: &Path, new: &Path) -> PathBuf {
    let from: Vec<_> = old.parent().unwrap_or(Path::new("/")).components().collect();
    let to: Vec<_> = new.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut target = PathBuf::new();
    for _ in common..from.len() {
        target.push("..");
    }
    for part in &to[common..] {
        target.push(part);
    }
    target
}

/// `old` é o link que a migração deixou (e não um link do usuário).
fn is_our_link(old: &Path, new: &Path) -> bool {
    fs::read_link(old).is_ok_and(|t| t == link_target(old, new) || t == new)
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// A pasta que vale para o par: a velha enquanto ela existe e não é o nosso link; senão, a nova.
pub fn pick(old: &Path, new: &Path) -> PathBuf {
    if old.is_dir() && !is_our_link(old, new) {
        old.to_path_buf()
    } else {
        new.to_path_buf()
    }
}

/// `~/.config/swaydesk` (ou a velha, pela regra do módulo).
pub fn config_dir() -> PathBuf {
    let b = Bases::from_env();
    pick(&b.config.join(OLD), &b.config.join(NEW))
}

/// `~/.local/share/swaydesk` (ou a velha).
pub fn data_dir() -> PathBuf {
    let b = Bases::from_env();
    pick(&b.data.join(OLD), &b.data.join(NEW))
}

/// `~/.local/state/swaydesk` (ou a velha, enquanto ela for uma pasta de verdade: a nova já existia antes da 0.1.4).
pub fn state_dir() -> PathBuf {
    let b = Bases::from_env();
    pick(&b.state.join(OLD), &b.state.join(NEW))
}

/// A pasta do `config.jsonc` da Waybar da sessão do pacote (`~/.config/swaydesk/waybar`, ou a velha).
pub fn waybar_dir() -> PathBuf {
    Bases::from_env().waybar_dir()
}

impl Bases {
    /// O `waybar_dir` destas bases: para onde o nosso link aponta (mesmo que o par da config tenha ficado no
    /// velho); senão, a velha de verdade; senão, a de dentro da config em uso.
    pub fn waybar_dir(&self) -> PathBuf {
        let old = self.config.join("swaydesk-desktop/waybar");
        let new = self.config.join(NEW).join("waybar");
        if is_our_link(&old, &new) {
            return new;
        }
        if old.is_dir() {
            return old;
        }
        pick(&self.config.join(OLD), &self.config.join(NEW)).join("waybar")
    }
}

/// `~/.cache/swaydesk`: o cache não migra, recomeça vazio no novo.
pub fn cache_dir() -> PathBuf {
    Bases::from_env().cache.join(NEW)
}

/// O que um par deu na migração.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// O velho é link ou não existe.
    Nothing,
    /// O velho foi renomeado (ou esvaziado no novo) e virou o link.
    Moved,
    /// Os dois têm itens com o mesmo nome: nada mudou, vale o velho.
    Both(Vec<String>),
    /// Uma operação falhou: o velho continua valendo.
    Failed(String),
}

/// Quantas vezes o velho pode ser recriado por fora entre o esvaziar e o link antes de desistir.
const MAX_ROUNDS: u32 = 2;

fn symlink_or_merge(old: &Path, new: &Path, round: u32) -> Outcome {
    match std::os::unix::fs::symlink(link_target(old, new), old) {
        Ok(()) => Outcome::Moved,
        // Alguém de fora da trava recriou o velho entre o rename e o link: os itens dele vão para o novo
        // Esgotadas as rodadas, o velho recriado fica como pasta (talvez vazia) e o pick o escolhe: só com alguém
        // recriando a pasta por fora da trava, no login, o que na prática não acontece
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && round < MAX_ROUNDS => merge(old, new, round + 1),
        Err(e) => Outcome::Failed(format!("{}: {e}", old.display())),
    }
}

fn merge(old: &Path, new: &Path, round: u32) -> Outcome {
    let entries = match fs::read_dir(old) {
        Ok(entries) => {
            let mut names = entries.filter_map(Result::ok).map(|e| e.file_name()).collect::<Vec<_>>();
            names.sort(); // uma ordem fixa: os testes e o desfazer
            names
        }
        Err(e) => return Outcome::Failed(format!("{}: {e}", old.display())),
    };
    let mut both: Vec<String> = entries
        .iter()
        .filter(|name| fs::symlink_metadata(new.join(name)).is_ok())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    if !both.is_empty() {
        both.sort();
        return Outcome::Both(both);
    }
    for (moved, name) in entries.iter().enumerate() {
        if let Err(e) = fs::rename(old.join(name), new.join(name)) {
            // Desfaz o que já foi: com o velho ainda pasta, o helper continua nele, e os itens movidos sumiriam
            let mut message = format!("{}: {e}", old.join(name).display());
            for back in entries[..moved].iter().rev() {
                if let Err(e) = fs::rename(new.join(back), old.join(back)) {
                    message.push_str(&format!("; and {} could not go back: {e}", new.join(back).display()));
                }
            }
            return Outcome::Failed(message);
        }
    }
    if let Err(e) = fs::remove_dir(old) {
        return Outcome::Failed(format!("{}: {e}", old.display()));
    }
    symlink_or_merge(old, new, round)
}

/// Migra um par (os casos do módulo). Sem trava: quem chama segura a de `migrate`.
pub fn migrate_pair(old: &Path, new: &Path) -> Outcome {
    if !is_real_dir(old) {
        return Outcome::Nothing;
    }
    if fs::symlink_metadata(new).is_err() {
        if let Some(parent) = new.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return Outcome::Failed(format!("{}: {e}", parent.display()));
            }
        }
        return match fs::rename(old, new) {
            Ok(()) => symlink_or_merge(old, new, 0),
            Err(e) => Outcome::Failed(format!("{} → {}: {e}", old.display(), new.display())),
        };
    }
    if !is_real_dir(new) {
        return Outcome::Failed(format!("{}: not a folder", new.display()));
    }
    merge(old, new, 0)
}

fn lock(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(path)?;
    // SAFETY: o fd é do File, que vive enquanto a trava; LOCK_EX espera o outro migrador terminar
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

/// Migra todos os pares sob a trava e devolve uma linha para o log por par que não migrou de vez (os dois
/// existem, ou algo falhou). Rodar de novo não muda nada.
pub fn migrate_with(bases: &Bases) -> Vec<String> {
    let _lock = match lock(&bases.lock_file()) {
        Ok(file) => file,
        Err(e) => return vec![format!("folders not migrated: lock {}: {e}", bases.lock_file().display())],
    };
    let mut messages = Vec::new();
    let mut config_in_use = true;
    for (index, (old, new)) in bases.pairs().into_iter().enumerate() {
        // A pasta da Waybar só vai para dentro da config nova se a config nova é a que vale
        if index == WAYBAR_PAIR && !config_in_use {
            if is_real_dir(&old) {
                messages.push(format!("{} kept: {} stays in use", old.display(), bases.config.join(OLD).display()));
            }
            continue;
        }
        let outcome = migrate_pair(&old, &new);
        if index == CONFIG_PAIR {
            config_in_use = matches!(outcome, Outcome::Nothing | Outcome::Moved) && !is_real_dir(&old);
        }
        match outcome {
            Outcome::Nothing | Outcome::Moved => {}
            Outcome::Both(names) => messages.push(format!(
                "{} kept: {} also exists in {}; the old folder stays in use until you merge them by hand",
                old.display(),
                names.join(", "),
                new.display()
            )),
            Outcome::Failed(err) => messages.push(format!("{} not migrated: {err}", old.display())),
        }
    }
    messages
}

/// `migrate_with` com as pastas do ambiente.
pub fn migrate() -> Vec<String> {
    migrate_with(&Bases::from_env())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Home(PathBuf);

    impl Home {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("swaydesk-dirs-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Home(dir)
        }
        fn bases(&self) -> Bases {
            Bases {
                config: self.0.join(".config"),
                data: self.0.join(".local/share"),
                state: self.0.join(".local/state"),
                cache: self.0.join(".cache"),
            }
        }
        fn write(&self, rel: &str, text: &str) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fn read(&self, rel: &str) -> Option<String> {
            fs::read_to_string(self.0.join(rel)).ok()
        }
        fn link(&self, rel: &str) -> Option<PathBuf> {
            fs::read_link(self.0.join(rel)).ok()
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
            for sub in [".config", ".local/state"] {
                let _ = fs::set_permissions(self.0.join(sub), fs::Permissions::from_mode(0o755));
            }
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn nothing_exists_nothing_happens() {
        let home = Home::new("none");
        assert!(migrate_with(&home.bases()).is_empty());
        assert!(!home.0.join(".config/swaydesk").exists());
        assert!(!home.0.join(".config/sway-desktop").exists());
    }

    #[test]
    fn only_the_old_one_moves_and_leaves_a_relative_link() {
        let home = Home::new("old");
        home.write(".config/sway-desktop/config.toml", "accent = \"#123456\"\n");
        home.write(".config/sway-desktop/displays.json", "{}");
        home.write(".local/share/sway-desktop/rofi/sway-desktop.rasi", "rasi");
        home.write(".config/swaydesk-desktop/waybar/config.jsonc", "{}");
        assert!(migrate_with(&home.bases()).is_empty());
        assert_eq!(home.read(".config/swaydesk/config.toml").as_deref(), Some("accent = \"#123456\"\n"));
        assert_eq!(home.link(".config/sway-desktop"), Some(PathBuf::from("swaydesk")));
        assert_eq!(home.link(".local/share/sway-desktop"), Some(PathBuf::from("swaydesk")));
        assert_eq!(home.link(".config/swaydesk-desktop/waybar"), Some(PathBuf::from("../swaydesk/waybar")));
        // O caminho velho ainda lê o mesmo arquivo (o rollback para a 0.1.3.x)
        assert_eq!(home.read(".config/sway-desktop/config.toml").as_deref(), Some("accent = \"#123456\"\n"));
        assert_eq!(home.read(".config/swaydesk-desktop/waybar/config.jsonc").as_deref(), Some("{}"));
        assert_eq!(home.read(".local/share/swaydesk/rofi/sway-desktop.rasi").as_deref(), Some("rasi"));
    }

    #[test]
    fn running_twice_changes_nothing() {
        let home = Home::new("twice");
        home.write(".config/sway-desktop/config.toml", "x");
        migrate_with(&home.bases());
        assert!(migrate_with(&home.bases()).is_empty());
        assert_eq!(home.link(".config/sway-desktop"), Some(PathBuf::from("swaydesk")));
        assert_eq!(home.read(".config/swaydesk/config.toml").as_deref(), Some("x"));
    }

    #[test]
    fn only_the_new_one_is_left_alone() {
        let home = Home::new("new");
        home.write(".config/swaydesk/config.toml", "new");
        assert!(migrate_with(&home.bases()).is_empty());
        assert!(fs::symlink_metadata(home.0.join(".config/sway-desktop")).is_err());
        assert_eq!(home.read(".config/swaydesk/config.toml").as_deref(), Some("new"));
    }

    #[test]
    fn the_state_folder_takes_the_old_items_next_to_its_own() {
        let home = Home::new("state");
        home.write(".local/state/swaydesk/language-pack-notice", "1");
        home.write(".local/state/sway-desktop/accent-auto", "#abcdef");
        home.write(".local/state/sway-desktop/antes-do-sway-theme/kitty.conf", "k");
        assert!(migrate_with(&home.bases()).is_empty());
        assert_eq!(home.read(".local/state/swaydesk/accent-auto").as_deref(), Some("#abcdef"));
        assert_eq!(home.read(".local/state/swaydesk/antes-do-sway-theme/kitty.conf").as_deref(), Some("k"));
        assert_eq!(home.read(".local/state/swaydesk/language-pack-notice").as_deref(), Some("1"));
        assert_eq!(home.link(".local/state/sway-desktop"), Some(PathBuf::from("swaydesk")));
    }

    #[test]
    fn both_with_the_same_item_change_nothing_and_warn() {
        let home = Home::new("both");
        home.write(".config/sway-desktop/config.toml", "old");
        home.write(".config/swaydesk/config.toml", "new");
        let messages = migrate_with(&home.bases());
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("config.toml"), "{messages:?}");
        assert_eq!(home.read(".config/sway-desktop/config.toml").as_deref(), Some("old"));
        assert_eq!(home.read(".config/swaydesk/config.toml").as_deref(), Some("new"));
        // Vale o velho, que a 0.1.3.x usava
        let b = home.bases();
        assert_eq!(pick(&b.config.join(OLD), &b.config.join(NEW)), b.config.join(OLD));
    }

    #[test]
    fn a_user_link_in_place_of_the_old_folder_is_respected() {
        let home = Home::new("stow");
        home.write("dotfiles/sway-desktop/config.toml", "mine");
        fs::create_dir_all(home.0.join(".config")).unwrap();
        std::os::unix::fs::symlink(home.0.join("dotfiles/sway-desktop"), home.0.join(".config/sway-desktop")).unwrap();
        assert!(migrate_with(&home.bases()).is_empty());
        assert!(!home.0.join(".config/swaydesk").exists());
        let b = home.bases();
        // O helper lê e grava pelo link do usuário, e nada cria um novo vazio que o esconda
        assert_eq!(pick(&b.config.join(OLD), &b.config.join(NEW)), b.config.join(OLD));
    }

    #[test]
    fn a_failed_rename_keeps_the_old_folder_in_use() {
        let home = Home::new("ro");
        home.write(".config/sway-desktop/config.toml", "x");
        let config = home.0.join(".config");
        fs::set_permissions(&config, fs::Permissions::from_mode(0o555)).unwrap();
        let messages = migrate_with(&home.bases());
        fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).unwrap();
        if unsafe { libc::geteuid() } == 0 {
            return; // o root renomeia mesmo assim
        }
        assert!(messages.iter().any(|m| m.contains("not migrated")), "{messages:?}");
        assert!(is_real_dir(&config.join("sway-desktop")));
        assert!(!config.join("swaydesk").exists());
        let b = home.bases();
        assert_eq!(pick(&b.config.join(OLD), &b.config.join(NEW)), b.config.join(OLD));
    }

    #[test]
    fn the_helper_in_its_four_states() {
        let home = Home::new("pick");
        let b = home.bases();
        let (old, new) = (b.config.join(OLD), b.config.join(NEW));
        assert_eq!(pick(&old, &new), new, "nenhuma");
        fs::create_dir_all(&old).unwrap();
        assert_eq!(pick(&old, &new), old, "só a velha");
        fs::create_dir_all(&new).unwrap();
        assert_eq!(pick(&old, &new), old, "as duas");
        fs::remove_dir(&old).unwrap();
        assert_eq!(pick(&old, &new), new, "só a nova");
        std::os::unix::fs::symlink("swaydesk", &old).unwrap();
        assert_eq!(pick(&old, &new), new, "a velha é o nosso link");
    }

    #[test]
    fn parents_go_before_the_nested_waybar_folder() {
        let home = Home::new("order");
        home.write(".config/sway-desktop/config.toml", "x");
        home.write(".config/swaydesk-desktop/waybar/config.jsonc", "{}");
        let pairs = home.bases().pairs();
        assert_eq!(pairs[0].1, home.0.join(".config/swaydesk"));
        assert_eq!(pairs.last().unwrap().1, home.0.join(".config/swaydesk/waybar"));
        assert!(migrate_with(&home.bases()).is_empty());
        assert_eq!(home.link(".config/sway-desktop"), Some(PathBuf::from("swaydesk")));
        assert_eq!(home.read(".config/swaydesk/waybar/config.jsonc").as_deref(), Some("{}"));
    }

    #[test]
    fn a_writer_after_a_failed_rename_writes_into_the_old_folder() {
        let home = Home::new("writer");
        home.write(".config/sway-desktop/config.toml", "x");
        let b = home.bases();
        // Sem migrar (como depois de um rename que falhou): o helper continua na velha
        let dir = pick(&b.config.join(OLD), &b.config.join(NEW));
        fs::write(dir.join("displays.json"), "{}").unwrap();
        assert_eq!(home.read(".config/sway-desktop/displays.json").as_deref(), Some("{}"));
        assert!(!b.config.join(NEW).exists());
    }

    #[test]
    fn two_migrators_at_once_leave_one_consistent_result() {
        let home = Home::new("race");
        for i in 0..50 {
            home.write(&format!(".config/sway-desktop/f{i}"), "x");
        }
        home.write(".local/state/swaydesk/own", "1");
        home.write(".local/state/sway-desktop/accent-auto", "#000000");
        let bases = home.bases();
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let b = bases.clone();
                std::thread::spawn(move || migrate_with(&b))
            })
            .collect();
        for t in threads {
            assert!(t.join().unwrap().is_empty());
        }
        assert_eq!(home.link(".config/sway-desktop"), Some(PathBuf::from("swaydesk")));
        assert_eq!(fs::read_dir(home.0.join(".config/swaydesk")).unwrap().count(), 50);
        assert_eq!(home.link(".local/state/sway-desktop"), Some(PathBuf::from("swaydesk")));
        assert_eq!(home.read(".local/state/swaydesk/accent-auto").as_deref(), Some("#000000"));
    }

    #[test]
    fn an_old_folder_recreated_before_the_link_is_merged() {
        let home = Home::new("eexist");
        let b = home.bases();
        let (old, new) = (b.config.join(OLD), b.config.join(NEW));
        home.write(".config/swaydesk/config.toml", "moved");
        home.write(".config/sway-desktop/displays.json", "{}");
        // O estado logo depois do rename, com o velho recriado por fora: o link dá EEXIST e os itens vão para o novo
        assert_eq!(symlink_or_merge(&old, &new, 0), Outcome::Moved);
        assert_eq!(home.read(".config/swaydesk/displays.json").as_deref(), Some("{}"));
        assert_eq!(home.link(".config/sway-desktop"), Some(PathBuf::from("swaydesk")));
    }

    #[test]
    fn a_config_clash_keeps_the_waybar_folder_next_to_the_config_in_use() {
        let home = Home::new("clash-waybar");
        home.write(".config/sway-desktop/config.toml", "old");
        home.write(".config/swaydesk/config.toml", "new");
        home.write(".config/swaydesk-desktop/waybar/config.jsonc", "{}");
        let messages = migrate_with(&home.bases());
        assert_eq!(messages.len(), 2, "{messages:?}");
        // A Waybar não foi para dentro da config nova, que não vale
        assert!(is_real_dir(&home.0.join(".config/swaydesk-desktop/waybar")));
        assert!(!home.0.join(".config/swaydesk/waybar").exists());
    }

    #[test]
    fn the_waybar_helper_follows_our_link() {
        let home = Home::new("waybar-link");
        home.write(".config/swaydesk/waybar/config.jsonc", "{}");
        home.write(".config/sway-desktop/config.toml", "old"); // a config ficou no velho
        fs::create_dir_all(home.0.join(".config/swaydesk-desktop")).unwrap();
        std::os::unix::fs::symlink("../swaydesk/waybar", home.0.join(".config/swaydesk-desktop/waybar")).unwrap();
        let b = home.bases();
        assert_eq!(b.waybar_dir(), b.config.join("swaydesk/waybar"));
        // Sem o link, a velha de verdade vale; sem nenhuma, a de dentro da config em uso
        fs::remove_file(b.config.join("swaydesk-desktop/waybar")).unwrap();
        assert_eq!(b.waybar_dir(), b.config.join("sway-desktop/waybar"));
        home.write(".config/swaydesk-desktop/waybar/config.jsonc", "{}");
        assert_eq!(b.waybar_dir(), b.config.join("swaydesk-desktop/waybar"));
    }

    #[test]
    fn a_merge_that_fails_halfway_puts_back_what_moved() {
        let home = Home::new("rollback");
        home.write(".local/state/swaydesk/own", "1");
        home.write(".local/state/sway-desktop/a-first", "a");
        home.write(".local/state/sway-desktop/z-locked/item", "z");
        // Uma pasta sem escrita não sai do lugar (o rename de pasta reescreve o "..")
        let locked = home.0.join(".local/state/sway-desktop/z-locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let b = home.bases();
        let outcome = migrate_pair(&b.state.join(OLD), &b.state.join(NEW));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        // O a-first (antes na ordem) foi e voltou
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        assert_eq!(home.read(".local/state/sway-desktop/a-first").as_deref(), Some("a"));
        assert!(!home.0.join(".local/state/swaydesk/a-first").exists());
        assert!(is_real_dir(&b.state.join(OLD)));
    }

    #[test]
    fn link_targets_are_relative() {
        let c = Path::new("/h/.config");
        assert_eq!(link_target(&c.join("sway-desktop"), &c.join("swaydesk")), PathBuf::from("swaydesk"));
        assert_eq!(
            link_target(&c.join("swaydesk-desktop/waybar"), &c.join("swaydesk/waybar")),
            PathBuf::from("../swaydesk/waybar")
        );
    }
}
