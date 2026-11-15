//! O glob dos `include` (*, ? e [...]), como o wordexp que o Sway usa: em ordem alfabética, sem os nomes
//! que começam com ponto (a não ser que o padrão também comece), e só o que existe.
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Quantos caminhos um padrão pode dar; o resto fica de fora (o limite de arquivos do parser vem depois).
const MAX_MATCHES: usize = 1024;

pub fn expand(path: &Path) -> Vec<PathBuf> {
    let mut current = vec![PathBuf::new()];
    for component in path.components() {
        let part = match component {
            Component::Normal(part) => part.to_string_lossy().into_owned(),
            other => {
                for p in &mut current {
                    p.push(other.as_os_str());
                }
                continue;
            }
        };
        if !has_wildcard(&part) {
            for p in &mut current {
                p.push(&part);
            }
            continue;
        }
        let mut next = Vec::new();
        for dir in &current {
            let Ok(read) = fs::read_dir(if dir.as_os_str().is_empty() { Path::new(".") } else { dir }) else {
                continue;
            };
            let mut names: Vec<String> = read
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| !name.starts_with('.') || part.starts_with('.'))
                .filter(|name| matches(&part, name))
                .collect();
            names.sort();
            next.extend(names.into_iter().map(|name| dir.join(name)));
            if next.len() >= MAX_MATCHES {
                next.truncate(MAX_MATCHES);
                break;
            }
        }
        current = next;
    }
    current.retain(|p| p.exists());
    current
}

fn has_wildcard(part: &str) -> bool {
    part.contains(['*', '?', '['])
}

/// O fnmatch: * (qualquer sequência), ? (um caractere) e [abc], [a-z], [!a] ou [^a].
pub fn matches(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    // O algoritmo de volta ao último *: linear na prática, sem recursão
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some((pi, ni));
                    pi += 1;
                    continue;
                }
                '?' => {
                    pi += 1;
                    ni += 1;
                    continue;
                }
                '[' => {
                    if let Some((hit, len)) = class(&p[pi..], n[ni]) {
                        if hit {
                            pi += len;
                            ni += 1;
                            continue;
                        }
                    } else if n[ni] == '[' {
                        pi += 1;
                        ni += 1;
                        continue;
                    }
                }
                c if c == n[ni] => {
                    pi += 1;
                    ni += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((sp, sn)) => {
                pi = sp + 1;
                ni = sn + 1;
                star = Some((sp, sn + 1));
            }
            None => return false,
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Um [...] no começo de `p`: se casa com `c` e o tamanho dele; None se o colchete não fecha.
fn class(p: &[char], c: char) -> Option<(bool, usize)> {
    let mut i = 1;
    let negate = matches!(p.get(i), Some('!') | Some('^'));
    if negate {
        i += 1;
    }
    let mut hit = false;
    let mut first = true;
    while i < p.len() {
        if p[i] == ']' && !first {
            return Some((hit != negate, i + 1));
        }
        first = false;
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            if p[i] <= c && c <= p[i + 2] {
                hit = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                hit = true;
            }
            i += 1;
        }
    }
    None
}
