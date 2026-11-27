//! O tema do cursor da sessão: o Bibata Modern Classic, que o Debian, o Ubuntu e o Mint empacotam; onde ele
//! não existe (o Arch só o tem no AUR), o Capitaine, o mais parecido nos repositórios oficiais; sem nenhum,
//! None e o padrão do sistema. O mesmo do `CURSOR_THEMES` do `sway_themes.py`.
use std::path::{Path, PathBuf};

use crate::config;

/// Na ordem de preferência.
pub const CURSOR_THEMES: [&str; 2] = ["Bibata-Modern-Classic", "capitaine-cursors"];
pub const CURSOR_SIZE: u32 = 24;

/// Onde o Xcursor e o GTK procuram os temas, do que vence para o que perde. SWAY_SYSTEM_ICONS troca o
/// /usr/share/icons nos testes.
pub fn icon_dirs() -> Vec<PathBuf> {
    let system = std::env::var("SWAY_SYSTEM_ICONS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/share/icons".into());
    vec![config::home().join(".icons"), config::data_home().join("icons"), PathBuf::from(system)]
}

/// O tema do cursor da sessão: o pedido (o cursor_theme do config), se ele tem cursores numa das pastas; senão, o
/// primeiro de CURSOR_THEMES com cursores. O cursor_theme do desktop_config.py.
pub fn cursor_theme_in(dirs: &[PathBuf], wanted: &str) -> Option<String> {
    let installed = |name: &str| dirs.iter().any(|dir| has_cursors(&dir.join(name)));
    if !wanted.is_empty() && installed(wanted) {
        return Some(wanted.to_string());
    }
    CURSOR_THEMES.into_iter().find(|name| installed(name)).map(str::to_string)
}

pub fn cursor_theme(wanted: &str) -> Option<String> {
    cursor_theme_in(&icon_dirs(), wanted)
}

/// Os tamanhos oferecidos quando o arquivo do cursor não se lê.
pub const FALLBACK_SIZES: [u32; 4] = [24, 32, 48, 64];
/// Quanto do arquivo se lê: a tabela vem logo depois do cabeçalho, e 64 KiB cabem 5 mil imagens.
const HEADER_BYTES: u64 = 64 * 1024;
/// O tipo das imagens na tabela do XCursor (o subtipo é o tamanho nominal).
const XCURSOR_IMAGE: u32 = 0xfffd_0002;

/// Os tamanhos nominais de um arquivo XCursor dentro do config::CURSOR_SIZES, em ordem e sem repetir; vazio se o
/// arquivo não é um XCursor. O cabeçalho: "Xcur", o tamanho dele, a versão e o número de entradas; cada entrada:
/// tipo, subtipo e posição, em u32 little-endian.
pub fn xcursor_sizes(data: &[u8]) -> Vec<u32> {
    let at = |i: usize| data.get(i..i.checked_add(4)?).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if data.get(..4) != Some(b"Xcur".as_slice()) {
        return Vec::new();
    }
    let (Some(header), Some(count)) = (at(4), at(12)) else { return Vec::new() };
    let mut sizes = std::collections::BTreeSet::new();
    for n in 0..count as usize {
        let entry = header as usize + n * 12;
        let (Some(kind), Some(size)) = (at(entry), at(entry + 4)) else { break };
        if kind == XCURSOR_IMAGE && config::CURSOR_SIZES.contains(&i64::from(size)) {
            sizes.insert(size);
        }
    }
    sizes.into_iter().collect()
}

/// Os tamanhos do cursor de um tema, pelo cursors/left_ptr da primeira pasta que o tem; FALLBACK_SIZES sem ele ou
/// sem nenhum tamanho legível. Um tema que herda os cursores (Inherits= no index.theme) também fica com os de
/// reserva: os CURSOR_THEMES trazem o left_ptr.
pub fn theme_sizes_in(dirs: &[PathBuf], theme: &str) -> Vec<u32> {
    use std::io::Read as _;
    for dir in dirs {
        let Ok(file) = std::fs::File::open(dir.join(theme).join("cursors/left_ptr")) else { continue };
        let mut data = Vec::new();
        if file.take(HEADER_BYTES).read_to_end(&mut data).is_ok() {
            let sizes = xcursor_sizes(&data);
            if !sizes.is_empty() {
                return sizes;
            }
        }
        break;
    }
    FALLBACK_SIZES.to_vec()
}

/// Os do tema do seat (cursor_theme, com o pedido no config); FALLBACK_SIZES sem nenhum.
pub fn theme_sizes(wanted: &str) -> Vec<u32> {
    let dirs = icon_dirs();
    cursor_theme_in(&dirs, wanted).map_or_else(|| FALLBACK_SIZES.to_vec(), |theme| theme_sizes_in(&dirs, &theme))
}

/// O tamanho da lista mais perto do pedido; no empate, o menor. A própria lista vazia devolve o pedido.
pub fn closest_size(sizes: &[u32], wanted: u32) -> u32 {
    sizes.iter().copied().min_by_key(|s| (s.abs_diff(wanted), *s)).unwrap_or(wanted)
}

/// Um tema de ícones sem a pasta cursors/ não é um tema de cursor.
fn has_cursors(theme: &Path) -> bool {
    theme.join("cursors").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swaydesk-cursor-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn with_cursors(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir.join(name).join("cursors")).unwrap();
    }

    #[test]
    fn prefers_bibata_then_capitaine_then_none() {
        let tmp = tmp("order");
        let (user, system) = (tmp.join("user"), tmp.join("system"));
        let dirs = [user.clone(), system.clone()];
        assert_eq!(cursor_theme_in(&dirs, ""), None);
        with_cursors(&system, "capitaine-cursors");
        assert_eq!(cursor_theme_in(&dirs, "").as_deref(), Some("capitaine-cursors"));
        with_cursors(&user, "Bibata-Modern-Classic");
        assert_eq!(cursor_theme_in(&dirs, "").as_deref(), Some("Bibata-Modern-Classic"));
        // O pedido vence quando tem cursores; sem eles, o automático
        assert_eq!(cursor_theme_in(&dirs, "Adwaita").as_deref(), Some("Bibata-Modern-Classic"));
        with_cursors(&system, "Adwaita");
        assert_eq!(cursor_theme_in(&dirs, "Adwaita").as_deref(), Some("Adwaita"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Um XCursor mínimo: o cabeçalho e uma entrada por (tipo, tamanho), sem as imagens.
    pub(crate) fn xcursor(entries: &[(u32, u32)]) -> Vec<u8> {
        let mut data = b"Xcur".to_vec();
        for n in [16, 0x1_0000, entries.len() as u32] {
            data.extend(n.to_le_bytes());
        }
        for (kind, size) in entries {
            for n in [*kind, *size, 0] {
                data.extend(n.to_le_bytes());
            }
        }
        data
    }

    #[test]
    fn the_sizes_come_from_the_image_entries() {
        let data = xcursor(&[(XCURSOR_IMAGE, 48), (XCURSOR_IMAGE, 24), (0xfffe_0001, 32), (XCURSOR_IMAGE, 24), (XCURSOR_IMAGE, 128)]);
        // O comentário (0xfffe0001) não conta; o 128 fica fora do cursor_size
        assert_eq!(xcursor_sizes(&data), [24, 48]);
        assert!(xcursor_sizes(b"not a cursor").is_empty());
        // Uma tabela que diz ter mais entradas do que o arquivo traz: só as que existem
        let mut short = xcursor(&[(XCURSOR_IMAGE, 32)]);
        short[12..16].copy_from_slice(&1000u32.to_le_bytes());
        assert_eq!(xcursor_sizes(&short), [32]);
        assert!(xcursor_sizes(&data[..10]).is_empty());
    }

    #[test]
    fn a_theme_without_a_readable_left_ptr_gets_the_fallback() {
        let tmp = tmp("sizes");
        with_cursors(&tmp, "T");
        assert_eq!(theme_sizes_in(&[tmp.clone()], "T"), FALLBACK_SIZES);
        std::fs::write(tmp.join("T/cursors/left_ptr"), xcursor(&[(XCURSOR_IMAGE, 32), (XCURSOR_IMAGE, 64)])).unwrap();
        assert_eq!(theme_sizes_in(&[tmp.clone()], "T"), [32, 64]);
        std::fs::write(tmp.join("T/cursors/left_ptr"), b"junk").unwrap();
        assert_eq!(theme_sizes_in(&[tmp.clone()], "T"), FALLBACK_SIZES);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn the_closest_size_prefers_the_smaller_on_a_tie() {
        assert_eq!(closest_size(&[24, 32, 48], 30), 32);
        assert_eq!(closest_size(&[24, 32, 48], 28), 24);
        assert_eq!(closest_size(&[24, 32, 48], 96), 48);
        assert_eq!(closest_size(&[], 40), 40);
    }

    #[test]
    fn an_icon_theme_without_cursors_does_not_count() {
        let tmp = tmp("icons");
        std::fs::create_dir_all(tmp.join("Bibata-Modern-Classic")).unwrap();
        std::fs::write(tmp.join("Bibata-Modern-Classic/index.theme"), "[Icon Theme]\n").unwrap();
        assert_eq!(cursor_theme_in(&[tmp.clone()], ""), None);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
