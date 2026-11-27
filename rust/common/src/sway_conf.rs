//! Os arquivos que os apps gravam no `~/.config/sway/config.d` (o 90-displays.conf das Telas, o 80-input.conf
//! dos painéis Teclado e Mouse e touchpad): onde ficam e como se gravam. O Sway os lê no login, depois do
//! 50-local.conf.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// O arquivo `name` no `config.d` do Sway: `$XDG_CONFIG_HOME/sway/config.d` (só um caminho absoluto), senão o
/// `~/.config/sway/config.d`.
pub fn config_d_file(name: &str) -> PathBuf {
    config_d_file_from(std::env::var_os("XDG_CONFIG_HOME").as_deref(), std::env::var_os("HOME").as_deref(), name)
}

/// O [`config_d_file`] com o ambiente à escolha (os testes).
pub fn config_d_file_from(xdg: Option<&OsStr>, home: Option<&OsStr>, name: &str) -> PathBuf {
    let base = xdg
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(home.unwrap_or(OsStr::new("/"))).join(".config"));
    base.join("sway").join("config.d").join(name)
}

/// Grava num temporário e troca de nome. Recusa (sem gravar) um `config.d`, um `sway` ou uma pasta de config
/// que é link para dentro de um repositório git: nos dotfiles, um `config.d` ligado ao repositório deixaria o
/// arquivo gerado dentro dele. Um link mais acima (o `/home → /var/home` do Silverblue, uma pasta pessoal noutro
/// disco) e uma pasta pessoal que é ela mesma um repositório são do usuário: grava.
pub fn save(file: &Path, text: &str) -> std::io::Result<()> {
    let Some(dir) = file.parent() else {
        return Err(std::io::Error::other(format!("{}: no folder", file.display())));
    };
    std::fs::create_dir_all(dir)?;
    // O config.d, o sway e a pasta de config (o ~/.config ou o XDG_CONFIG_HOME)
    for folder in dir.ancestors().take(3) {
        let linked = std::fs::symlink_metadata(folder).is_ok_and(|m| m.file_type().is_symlink());
        if !linked {
            continue;
        }
        let real = std::fs::canonicalize(folder)?;
        if real.ancestors().any(|a| a.join(".git").exists()) {
            return Err(std::io::Error::other(format!(
                "{} is a link into the git tree {}: not writing there",
                folder.display(),
                real.display()
            )));
        }
    }
    let tmp = file.with_extension(format!("conf.{}.tmp", std::process::id()));
    let result = (|| {
        let mut out = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut out, text.as_bytes())?;
        out.sync_all()?;
        std::fs::rename(&tmp, file)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn temp(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("sway-conf-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    #[test]
    fn the_file_is_in_the_sway_config_d() {
        let file = |xdg: Option<&str>, home: Option<&str>| {
            config_d_file_from(xdg.map(OsStr::new), home.map(OsStr::new), "80-input.conf")
        };
        assert_eq!(file(None, Some("/home/u")), Path::new("/home/u/.config/sway/config.d/80-input.conf"));
        assert_eq!(file(Some("/x/cfg"), Some("/home/u")), Path::new("/x/cfg/sway/config.d/80-input.conf"));
        assert_eq!(file(Some("rel"), Some("/home/u")), Path::new("/home/u/.config/sway/config.d/80-input.conf"));
        assert_eq!(file(None, None), Path::new("/.config/sway/config.d/80-input.conf"));
    }

    #[test]
    fn saves_atomically_creating_the_folder() {
        let dir = temp("save");
        let file = dir.0.join("sway/config.d/80-input.conf");
        save(&file, "one\n").unwrap();
        save(&file, "two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "two\n");
        let names: Vec<_> = std::fs::read_dir(file.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("80-input.conf")], "no temporary left behind");
    }

    #[test]
    fn a_config_d_linked_to_a_plain_folder_is_written() {
        let dir = temp("link-plain");
        let real = dir.0.join("elsewhere");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(dir.0.join("sway")).unwrap();
        std::os::unix::fs::symlink(&real, dir.0.join("sway/config.d")).unwrap();
        save(&dir.0.join("sway/config.d/80-input.conf"), "x\n").unwrap();
        assert_eq!(std::fs::read_to_string(real.join("80-input.conf")).unwrap(), "x\n");
    }

    #[test]
    fn a_config_d_linked_into_a_git_tree_is_refused() {
        let dir = temp("link-git");
        let repo = dir.0.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("home/.config/sway/config.d")).unwrap();
        std::fs::create_dir_all(dir.0.join("home/.config/sway")).unwrap();
        std::os::unix::fs::symlink(repo.join("home/.config/sway/config.d"), dir.0.join("home/.config/sway/config.d"))
            .unwrap();
        let err = save(&dir.0.join("home/.config/sway/config.d/80-input.conf"), "x\n").unwrap_err();
        assert!(err.to_string().contains("git"), "{err}");
        assert!(!repo.join("home/.config/sway/config.d/80-input.conf").exists());
        // Um worktree tem um arquivo .git, não uma pasta
        let wt = dir.0.join("wt");
        std::fs::create_dir_all(wt.join("config.d")).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: /elsewhere\n").unwrap();
        std::fs::create_dir_all(dir.0.join("home2/sway")).unwrap();
        std::os::unix::fs::symlink(wt.join("config.d"), dir.0.join("home2/sway/config.d")).unwrap();
        assert!(save(&dir.0.join("home2/sway/config.d/80-input.conf"), "x\n").is_err());
    }

    #[test]
    fn a_git_home_reached_through_a_linked_parent_is_written() {
        // /home → /var/home (Silverblue), ou uma pasta pessoal noutro disco: o link é do caminho, não do config.d
        let dir = temp("linked-home");
        let real = dir.0.join("real-home");
        std::fs::create_dir_all(real.join(".git")).unwrap();
        std::os::unix::fs::symlink(&real, dir.0.join("link-home")).unwrap();
        save(&dir.0.join("link-home/.config/sway/config.d/80-input.conf"), "x\n").unwrap();
        assert!(real.join(".config/sway/config.d/80-input.conf").exists());
    }

    #[test]
    fn a_home_that_is_itself_a_git_tree_is_written() {
        let dir = temp("home-git");
        std::fs::create_dir_all(dir.0.join(".git")).unwrap();
        save(&dir.0.join(".config/sway/config.d/80-input.conf"), "x\n").unwrap();
    }
}
