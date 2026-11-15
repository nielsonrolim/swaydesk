//! Os arquivos de referência do repositório nos testes de paridade (os `.py` de `home/.local/bin` que os
//! testes comparam com o Rust). O tarball de origem dos pacotes (`packaging/make-orig.sh`) e o espelho público
//! só levam o que os pacotes instalam: lá, sem o arquivo, o teste passa com um aviso no stderr. No repositório
//! completo, que se reconhece pelo `install.sh` na raiz (os dois deixam de fora), um arquivo que sumiu é erro,
//! para o teste não virar um no-op sem ninguém ver.
use std::path::{Path, PathBuf};

/// O caminho `rel`, relativo à raiz do repositório, para um crate em `rust/<crate>` (`manifest_dir` é o
/// `env!("CARGO_MANIFEST_DIR")`); `None` no tarball dos pacotes e no espelho, quando o arquivo não está lá.
pub fn repo_file(manifest_dir: &str, rel: &str) -> Option<PathBuf> {
    let root = Path::new(manifest_dir).join("../..");
    let path = root.join(rel);
    if path.exists() {
        return Some(path);
    }
    assert!(!root.join("install.sh").exists(), "{rel}: reference file missing from the repository");
    // Direto no stderr, não pelo eprintln!: o libtest captura a saída dos testes que passam, e o aviso precisa
    // aparecer no log do build do pacote
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "skipped: the reference file {rel} is not in this source tree (the package tarball or the mirror)");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uma árvore por teste (`name`): os testes rodam em paralelo
    fn tree(name: &str, full: bool) -> PathBuf {
        let root = std::env::temp_dir().join(format!("swaydesk-reference-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("rust/crate")).unwrap();
        std::fs::write(root.join("here.py"), "").unwrap();
        // O espelho público também tem um .git: o que marca o repositório completo é o install.sh
        std::fs::write(root.join(".git"), "gitdir: elsewhere\n").unwrap();
        if full {
            std::fs::write(root.join("install.sh"), "").unwrap();
        }
        root
    }

    #[test]
    fn present_file() {
        let root = tree("present", false);
        let manifest = root.join("rust/crate");
        assert_eq!(repo_file(manifest.to_str().unwrap(), "here.py"), Some(manifest.join("../../here.py")));
    }

    #[test]
    fn missing_in_the_tarball_or_the_mirror_is_skipped() {
        let root = tree("skipped", false);
        assert_eq!(repo_file(root.join("rust/crate").to_str().unwrap(), "gone.py"), None);
    }

    #[test]
    #[should_panic(expected = "gone.py: reference file missing from the repository")]
    fn missing_in_the_repository_fails() {
        let root = tree("missing", true);
        repo_file(root.join("rust/crate").to_str().unwrap(), "gone.py");
    }
}
