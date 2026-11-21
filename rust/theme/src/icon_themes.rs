//! Os temas de ícones que dá para escolher: os de verdade, instalados nas pastas de ícones (a primeira que tem o
//! tema vence, como no GTK). Ficam de fora os ocultos, os só de cursor, os de base (hicolor, default, locolor) e o
//! sway-desktop-icons gerado. Os três Papirus viram uma entrada só, "Papirus", que segue o estilo do tema.
//! Os casos ficam em icon_themes.json, lidos também pelos testes do lado em Python.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::desktop::{self as dc, ICONS_WRAPPER};
use crate::papirus as sp;

/// A entrada única dos três Papirus.
pub const PAPIRUS: &str = "Papirus";
/// Os de base, que não são temas para escolher.
const BASES: [&str; 3] = ["hicolor", "default", "locolor"];

#[derive(Clone, Debug, PartialEq)]
pub struct IconTheme {
    /// O nome da pasta (PAPIRUS para os três)
    pub id: String,
    /// O Name do index.theme, na língua, ou o id
    pub label: String,
}

/// As chaves da seção [Icon Theme], na ordem do arquivo.
fn section(text: &str) -> Vec<(String, String)> {
    let mut keys = Vec::new();
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[Icon Theme]";
        } else if inside {
            if let Some((key, value)) = line.split_once('=') {
                keys.push((key.trim().to_string(), value.trim().to_string()));
            }
        }
    }
    keys
}

fn value<'a>(keys: &'a [(String, String)], key: &str) -> Option<&'a str> {
    keys.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Name[pt_BR], depois Name[pt], depois Name.
fn label(keys: &[(String, String)], lang: &str) -> Option<String> {
    let base = lang.split('.').next().unwrap_or("").split('@').next().unwrap_or("");
    let short = base.split('_').next().unwrap_or("");
    let mut wanted = Vec::new();
    if !base.is_empty() {
        wanted.push(format!("Name[{base}]"));
    }
    if !short.is_empty() && short != base {
        wanted.push(format!("Name[{short}]"));
    }
    wanted.push("Name".into());
    wanted.iter().find_map(|w| value(keys, w)).filter(|v| !v.is_empty()).map(str::to_string)
}

/// Um tema de ícones de verdade: não oculto, com algum diretório de Directories (ou ScaledDirectories) que
/// existe e não é o dos cursores.
fn real(dir: &Path, keys: &[(String, String)]) -> bool {
    if value(keys, "Hidden").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
        return false;
    }
    ["Directories", "ScaledDirectories"].iter().filter_map(|k| value(keys, k)).flat_map(|v| v.split(',')).map(str::trim).any(|sub| {
        !sub.is_empty() && sub.split('/').next() != Some("cursors") && dir.join(sub).is_dir()
    })
}

/// Os temas das pastas dirs (da que vence para a que perde), na ordem de mostrar: o Papirus primeiro, depois
/// pelo nome, sem diferença de maiúsculas.
pub fn list(dirs: &[PathBuf], lang: &str) -> Vec<IconTheme> {
    let mut seen = HashSet::new();
    let mut themes = Vec::new();
    let mut papirus = false;
    for base in dirs {
        let Ok(entries) = std::fs::read_dir(base) else { continue };
        let mut names: Vec<String> = entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
        names.sort();
        for name in names {
            let dir = base.join(&name);
            // Sem index.theme nesta pasta, a próxima pode ter o tema
            let Ok(text) = std::fs::read_to_string(dir.join("index.theme")) else { continue };
            if !seen.insert(name.clone()) {
                continue; // a primeira pasta com o tema vence, mesmo oculto
            }
            if name.starts_with('.') || name == ICONS_WRAPPER || BASES.contains(&name.as_str()) {
                continue;
            }
            let keys = section(&text);
            if !real(&dir, &keys) {
                continue;
            }
            if sp::is_papirus(&name) {
                papirus = true;
                continue;
            }
            let label = label(&keys, lang).unwrap_or_else(|| name.clone());
            themes.push(IconTheme { id: name, label });
        }
    }
    themes.sort_by_key(|t| (t.label.to_lowercase(), t.id.clone()));
    if papirus {
        themes.insert(0, IconTheme { id: PAPIRUS.into(), label: PAPIRUS.into() });
    }
    themes
}

/// Os instalados, nas pastas de ícones de sempre.
pub fn installed(lang: &str) -> Vec<IconTheme> {
    list(&dc::icon_dirs(), lang)
}

/// A entrada de um nome: os três Papirus são a mesma.
pub fn entry_of(name: &str) -> &str {
    if sp::is_papirus(name) { PAPIRUS } else { name }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: &str = include_str!("icon_themes.json");

    /// Monta as pastas de um caso do JSON num diretório temporário.
    fn build(root: &Path, dirs: &[serde_json::Value]) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for (i, d) in dirs.iter().enumerate() {
            let base = root.join(format!("d{i}"));
            std::fs::create_dir_all(&base).unwrap();
            for (name, theme) in d["themes"].as_object().unwrap() {
                let dir = base.join(name);
                std::fs::create_dir_all(&dir).unwrap();
                if let Some(index) = theme["index"].as_str() {
                    std::fs::write(dir.join("index.theme"), index).unwrap();
                }
                for sub in theme["subdirs"].as_array().into_iter().flatten() {
                    std::fs::create_dir_all(dir.join(sub.as_str().unwrap())).unwrap();
                }
            }
            paths.push(base);
        }
        paths
    }

    #[test]
    fn cases_shared_with_python() {
        let cases: serde_json::Value = serde_json::from_str(CASES).unwrap();
        for (i, case) in cases["cases"].as_array().unwrap().iter().enumerate() {
            let root = std::env::temp_dir().join(format!("swaydesk-icon-themes-{}-{i}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let dirs = build(&root, case["dirs"].as_array().unwrap());
            let got: Vec<(String, String)> = list(&dirs, case["lang"].as_str().unwrap_or("")).into_iter().map(|t| (t.id, t.label)).collect();
            let expected: Vec<(String, String)> = case["expected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| (e[0].as_str().unwrap().to_string(), e[1].as_str().unwrap().to_string()))
                .collect();
            let _ = std::fs::remove_dir_all(&root);
            assert_eq!(got, expected, "{}", case["name"]);
        }
    }

    #[test]
    fn papirus_is_one_entry() {
        for name in ["Papirus", "Papirus-Dark", "Papirus-Light"] {
            assert_eq!(entry_of(name), PAPIRUS);
        }
        assert_eq!(entry_of("Yaru"), "Yaru");
    }
}
