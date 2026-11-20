//! As cores das pastas do Papirus, como as do papirus-folders, mas só para o usuário e sem sudo. O porte
//! do `sway_papirus.py`: o tema sway-desktop-icons, em ~/.local/share/icons, herda o Papirus e tem só os
//! links das pastas na cor escolhida, com a mesma regra do change_color dele.
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use swaydesk_common::config::{data_home, linear_rgb, papirus_color, PAPIRUS_COLORS};

use crate::desktop::{icon_dirs, ini_value, ICONS_BASE_KEY, ICONS_WRAPPER};

pub const THEMES: &[&str] = &["Papirus", "Papirus-Dark", "Papirus-Light"];
/// Os tamanhos que o papirus-folders troca. Os @2x do Papirus são links para estes
const COLORED_SIZES: &[&str] = &["22x22", "24x24", "32x32", "48x48", "64x64"];
/// Os menores ficam com as pastas do próprio Papirus, como no papirus-folders
const ORIGINAL_SIZES: &[&str] = &["16x16"];
/// No index.theme: o que o gerou. Igual, nada a refazer
const SOURCE_KEY: &str = "X-SwayDesktop-Source";
/// Muda quando o wrapper_files passa a gerar outra coisa
const FORMAT: &str = "1";
/// Abaixo disso, no Lab, a cor é um cinza: o matiz não diz nada
pub(crate) const GREY_CHROMA: f64 = 12.0;

pub fn is_papirus(name: &str) -> bool {
    THEMES.contains(&name)
}

pub fn theme_dir(name: &str) -> Option<PathBuf> {
    icon_dirs().into_iter().map(|base| base.join(name)).find(|dir| dir.join("index.theme").is_file())
}

/// As cores que o Papirus instalado tem, achadas como o papirus-folders acha, na ordem da tabela.
pub fn available_colors(directory: Option<&Path>) -> Vec<String> {
    let Some(directory) = directory else { return Vec::new() };
    let places = directory.join("48x48/places");
    PAPIRUS_COLORS
        .iter()
        .filter(|(name, _)| places.join(format!("folder-{name}-documents.svg")).exists())
        .map(|(name, _)| name.to_string())
        .collect()
}

/// CIELAB (D65) do hex.
fn lab(color: &str) -> (f64, f64, f64) {
    lab_linear(linear_rgb(color))
}

/// CIELAB (D65) dos canais sem a curva do sRGB (o linear_rgb).
pub(crate) fn lab_linear([r, g, b]: [f64; 3]) -> (f64, f64, f64) {
    let xyz = [
        (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047,
        0.2126 * r + 0.7152 * g + 0.0722 * b,
        (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883,
    ];
    let [fx, fy, fz] = xyz.map(|v| if v > 0.008856 { v.powf(1.0 / 3.0) } else { 7.787 * v + 16.0 / 116.0 });
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

pub(crate) fn lch(color: &str) -> (f64, f64, f64) {
    lch_of_lab(lab(color))
}

/// O LCh (claridade, croma, matiz em graus) do Lab.
pub(crate) fn lch_of_lab((lightness, a, b): (f64, f64, f64)) -> (f64, f64, f64) {
    (lightness, (a * a + b * b).sqrt(), b.atan2(a).to_degrees().rem_euclid(360.0))
}

/// A distância para achar a variante de um tema: entre cores, vale o matiz. Cinza só fica perto de cinza.
pub fn distance(one: &str, other: &str) -> f64 {
    distance_lch(lch(one), lch(other))
}

/// O distance com as cores já em LCh.
pub(crate) fn distance_lch((l1, c1, h1): (f64, f64, f64), (l2, c2, h2): (f64, f64, f64)) -> f64 {
    if (c1 < GREY_CHROMA) != (c2 < GREY_CHROMA) {
        return 1000.0 + (l1 - l2).abs() + (c1 - c2).abs();
    }
    if c1 < GREY_CHROMA {
        return ((l1 - l2).powi(2) + (c1 - c2).powi(2)).sqrt();
    }
    let hue = (h1 - h2).abs().min(360.0 - (h1 - h2).abs());
    (hue.powi(2) + ((c1 - c2) / 2.0).powi(2) + ((l1 - l2) / 4.0).powi(2)).sqrt()
}

/// O nome da cor do Papirus mais perto do hex. Sem nomes, entre todas. No empate, a primeira (o min do Python).
pub fn nearest_color(color: &str, names: &[String]) -> String {
    let all: Vec<String>;
    let names = if names.is_empty() {
        all = PAPIRUS_COLORS.iter().map(|(n, _)| n.to_string()).collect();
        &all
    } else {
        names
    };
    let mut best: Option<(&String, f64)> = None;
    for name in names {
        let d = distance(color, papirus_color(name).unwrap_or("#000000"));
        if best.is_none_or(|(_, b)| d < b) {
            best = Some((name, d));
        }
    }
    best.map(|(n, _)| n.clone()).unwrap_or_default()
}

fn is_symlink(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink())
}

/// Segue os links do nome dentro da pasta. Devolve o arquivo colorido em que ele cai, ou None.
fn resolve_in(directory: &Path, name: &str, changed: &BTreeMap<String, String>) -> Option<String> {
    let mut name = name.to_string();
    for _ in 0..10 {
        if let Some(colored) = changed.get(&name) {
            return Some(colored.clone());
        }
        let path = directory.join(&name);
        if !is_symlink(&path) {
            return None;
        }
        let target = std::fs::read_link(&path).ok()?.to_string_lossy().into_owned();
        if target.contains('/') {
            return None;
        }
        name = target;
    }
    None
}

fn sorted_entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|it| it.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// {caminho relativo no tema gerado: arquivo de destino}.
pub fn folder_links(directory: &Path, color: &str) -> BTreeMap<String, String> {
    let mut links = BTreeMap::new();
    for size in COLORED_SIZES {
        let Ok(places) = directory.join(size).join("places").canonicalize() else { continue };
        if !places.is_dir() {
            continue;
        }
        let entries = sorted_entries(&places);
        let mut changed = BTreeMap::new();
        for prefix in [format!("folder-{color}"), format!("user-{color}")] {
            for file in entries.iter().filter(|n| n.starts_with(&prefix) && n.ends_with(".svg")) {
                let rest = &file[prefix.len()..file.len() - 4];
                if is_symlink(&places.join(file)) || (!rest.is_empty() && !rest.starts_with('-')) {
                    continue;
                }
                changed.insert(file.replacen(&format!("-{color}"), "", 1), file.clone());
            }
        }
        for entry in &entries {
            if let Some(colored) = resolve_in(&places, entry, &changed) {
                if *entry != colored {
                    links.insert(format!("{size}/places/{entry}"), places.join(&colored).to_string_lossy().into_owned());
                }
            }
        }
    }
    let names: BTreeSet<String> = links.keys().filter_map(|rel| rel.rsplit('/').next().map(str::to_string)).collect();
    for size in ORIGINAL_SIZES {
        let Ok(places) = directory.join(size).join("places").canonicalize() else { continue };
        for name in &names {
            let path = places.join(name);
            if path.exists() {
                let target = path.canonicalize().unwrap_or(path);
                links.insert(format!("{size}/places/{name}"), target.to_string_lossy().into_owned());
            }
        }
    }
    links
}

/// As seções [N/places] e [N@2x/places] do index.theme do Papirus, como estão lá.
fn directory_sections(directory: &Path, sizes: &BTreeSet<String>) -> BTreeMap<String, String> {
    let Ok(bytes) = std::fs::read(directory.join("index.theme")) else { return BTreeMap::new() };
    let text = String::from_utf8_lossy(&bytes);
    let mut sections = BTreeMap::new();
    // O `^\[([^\]\n]+)\]\n(.*?)(?=^\[|\Z)` do Python: o corpo vai até a próxima linha que começa com [
    let mut current: Option<(String, String)> = None;
    let mut flush = |section: Option<(String, String)>| {
        if let Some((name, body)) = section {
            let size = name.split('/').next().unwrap_or("").trim_end_matches("@2x").to_string();
            if name.ends_with("/places") && name.matches('/').count() == 1 && sizes.contains(&size) {
                sections.insert(name, body.trim().to_string());
            }
        }
    };
    for line in text.split_inclusive('\n') {
        let header = line.strip_suffix('\n').and_then(|l| l.strip_prefix('[')).and_then(|l| l.strip_suffix(']'));
        if let Some(name) = header.filter(|n| !n.is_empty() && !n.contains(']')) {
            flush(current.take());
            current = Some((name.to_string(), String::new()));
        } else if line.starts_with('[') {
            flush(current.take()); // um [ que não abre seção válida ainda termina a anterior
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
        }
    }
    flush(current.take());
    sections
}

/// O que define o tema gerado: o tema, a cor, a herança e a data das pastas do Papirus.
fn source(base: &str, inherits: &[String], color: Option<&str>) -> String {
    let directory = color.and_then(|_| theme_dir(base));
    let mut parts = vec![FORMAT.to_string(), base.to_string(), color.unwrap_or("").to_string(), inherits.join(",")];
    parts.push(directory.as_ref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default());
    if let Some(directory) = &directory {
        for size in COLORED_SIZES.iter().chain(ORIGINAL_SIZES) {
            let stamp = std::fs::metadata(directory.join(size).join("places"))
                .map(|m| (m.mtime() as i128 * 1_000_000_000 + m.mtime_nsec() as i128).to_string())
                .unwrap_or_else(|_| "-".into());
            parts.push(stamp);
        }
    }
    parts.join("|")
}

pub struct WrapperFiles {
    pub index: String,
    pub links: BTreeMap<String, String>,
    pub scaled: BTreeMap<String, String>,
}

/// O index.theme e os links do tema gerado. Sem cor (ou sem Papirus), só a herança.
pub fn wrapper_files(base: &str, inherits: &[String], color: Option<&str>) -> WrapperFiles {
    let directory = color.and_then(|_| theme_dir(base));
    let links = match (&directory, color) {
        (Some(dir), Some(c)) if available_colors(Some(dir)).iter().any(|n| n == c) => folder_links(dir, c),
        _ => BTreeMap::new(),
    };
    let sizes: BTreeSet<String> = links.keys().map(|rel| rel.split('/').next().unwrap_or("").to_string()).collect();
    let sections = match (&directory, links.is_empty()) {
        (Some(dir), false) => directory_sections(dir, &sizes),
        _ => BTreeMap::new(),
    };
    let first = |name: &str| name.split('/').next().unwrap_or("").to_string();
    let folders: Vec<&String> = sections
        .keys()
        .filter(|name| sizes.contains(&first(name)) || sizes.contains(first(name).trim_end_matches("@2x")))
        .collect();
    let mut index = format!(
        "[Icon Theme]\nName={ICONS_WRAPPER}\nComment={base}{}\nHidden=true\nInherits={}\n{ICONS_BASE_KEY}={base}\n{SOURCE_KEY}={}\n",
        if links.is_empty() { String::new() } else { format!(", pastas {}", color.unwrap_or("")) },
        inherits.join(","),
        source(base, inherits, color)
    );
    if folders.is_empty() {
        // Sem diretórios, o GTK ignoraria o tema inteiro, com a herança: um vazio basta
        index.push_str("Directories=vazio\n\n[vazio]\nSize=16\nType=Threshold\n");
    } else {
        index.push_str(&format!("Directories={}\n", folders.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")));
        for name in &folders {
            index.push_str(&format!("\n[{name}]\n{}\n", sections[*name]));
        }
    }
    let scaled = folders
        .iter()
        .filter(|name| name.contains("@2x"))
        .map(|name| (first(name), first(name).trim_end_matches("@2x").to_string()))
        .collect();
    WrapperFiles { index, links, scaled }
}

pub fn root() -> PathBuf {
    data_home().join("icons").join(ICONS_WRAPPER)
}

fn current_source() -> Option<String> {
    let text = std::fs::read_to_string(root().join("index.theme")).ok()?;
    // O `^X=(.*)$` do Python: o valor sem tirar os espaços
    text.lines().find_map(|line| line.strip_prefix(SOURCE_KEY)?.strip_prefix('=').map(str::to_string))
}

/// As pastas reais do tema gerado: o nome dele é um link para uma delas.
fn versions() -> Vec<PathBuf> {
    let Some(parent) = root().parent().map(Path::to_path_buf) else { return Vec::new() };
    let prefix = format!(".{ICONS_WRAPPER}-");
    let mut found: Vec<PathBuf> =
        sorted_entries(&parent).into_iter().filter(|n| n.starts_with(&prefix)).map(|n| parent.join(n)).collect();
    found.sort();
    found
}

fn remove_tree(path: &Path) {
    if is_symlink(path) || path.is_file() {
        let _ = std::fs::remove_file(path);
    } else {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// Gera o tema, se mudou. O nome é um link para uma pasta completa, trocado com um rename. Quem chama
/// segura a trava do sway-theme.
pub fn build(base: &str, inherits: &[String], color: Option<&str>) -> std::io::Result<()> {
    if current_source().as_deref() == Some(source(base, inherits, color).as_str()) {
        return Ok(());
    }
    let files = wrapper_files(base, inherits, color);
    let link = root();
    let parent = link.parent().map(Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&parent)?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let pid = std::process::id();
    let folder = parent.join(format!(".{ICONS_WRAPPER}-{nanos}-{pid}"));
    let tmp = parent.join(format!(".{ICONS_WRAPPER}.link.{pid}"));
    let made = (|| -> std::io::Result<()> {
        std::fs::create_dir(&folder)?;
        std::fs::write(folder.join("index.theme"), &files.index)?;
        for (rel, target) in &files.links {
            let path = folder.join(rel);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::os::unix::fs::symlink(target, &path)?;
        }
        for (name, target) in &files.scaled {
            std::os::unix::fs::symlink(target, folder.join(name))?;
        }
        let _ = std::fs::remove_file(&tmp);
        std::os::unix::fs::symlink(folder.file_name().unwrap_or_default(), &tmp)?;
        if link.is_dir() && !is_symlink(&link) {
            std::fs::rename(&link, parent.join(format!(".{ICONS_WRAPPER}-antigo-{pid}")))?;
        }
        std::fs::rename(&tmp, &link)
    })();
    if let Err(err) = made {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_dir_all(&folder);
        return Err(err);
    }
    for old in versions() {
        if old != folder {
            remove_tree(&old);
        }
    }
    Ok(())
}

pub fn remove() {
    let link = root();
    if is_symlink(&link) {
        let _ = std::fs::remove_file(&link);
    } else if link.is_dir() {
        let _ = std::fs::remove_dir_all(&link);
    }
    for old in versions() {
        remove_tree(&old);
    }
}

/// O tema que o tema gerado herda primeiro (o mesmo do desktop::icons_wrapper_base).
pub fn base_of_index(text: &str) -> Option<String> {
    ini_value(text, ICONS_BASE_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_like_python() {
        // Conferidos com o sway_papirus.py
        assert_eq!(nearest_color("#199ca8", &[]), "darkcyan");
        assert_eq!(nearest_color("#e82127", &[]), "carmine");
        assert_eq!(nearest_color("#808080", &[]), "grey");
        assert_eq!(nearest_color("#199ca8", &["red".into(), "blue".into()]), "blue");
    }

    #[test]
    fn directory_sections_like_python() {
        let dir = std::env::temp_dir().join(format!("swaydesk-theme-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.theme"),
            "[Icon Theme]\nName=P\n\n[22x22/places]\nSize=22\nContext=Places\n\n[22x22@2x/places]\nSize=22\nScale=2\n\n[22x22/apps]\nSize=22\n\n[48x48/places]\nSize=48\n",
        )
        .unwrap();
        let sizes: BTreeSet<String> = ["22x22".to_string()].into();
        let found = directory_sections(&dir, &sizes);
        assert_eq!(found.keys().collect::<Vec<_>>(), ["22x22/places", "22x22@2x/places"]);
        assert_eq!(found["22x22/places"], "Size=22\nContext=Places");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
