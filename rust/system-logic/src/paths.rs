//! Os caminhos como o `os.path` do Python: o realpath que aceita o que não existe, o abspath que só
//! limpa o texto e o expanduser.
use std::path::{Component, Path, PathBuf};

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into())
}

/// O `os.path.abspath`: absoluto e sem `.` e `..`, sem resolver links.
pub fn abspath(path: &Path) -> PathBuf {
    let path = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) };
    let mut out = PathBuf::from("/");
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => out.push(name),
            _ => {}
        }
    }
    out
}

/// O `os.path.realpath`: resolve os links do que existe e mantém o resto (o Python não exige que exista).
/// Duas diferenças do Python, sem efeito no `Project::owns`:
/// - um link quebrado fica como está, e o Python segue o alvo dele;
/// - o `..` é limpo no texto antes de resolver os links, e o Python resolve o link e depois sobe (só
///   muda com `link/../x`, com o link apontando para outra pasta).
pub fn realpath(path: &Path) -> PathBuf {
    let abs = abspath(path);
    let mut existing = abs.clone();
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            let mut out = real;
            for part in rest.iter().rev() {
                out.push(part);
            }
            // O resto pode ter "..": limpa de novo
            return abspath(&out);
        }
        match (existing.file_name().map(|n| n.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return abs,
        }
    }
}

/// O `os.path.expanduser`, só para o próprio usuário (~ e ~/...).
pub fn expanduser(word: &str) -> PathBuf {
    if word == "~" {
        return home();
    }
    match word.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(word),
    }
}
