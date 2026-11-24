//! A cor de destaque tirada do papel de parede (accent_auto, ligado por padrão): a cor do Papirus mais perto da cor
//! dominante da imagem. O sway_wallpaper_accent.py faz o mesmo, com as mesmas constantes e os mesmos vetores de teste.
//! Sem cor na imagem, ou sem imagem legível, quem chama usa o DEFAULT_ACCENT.
//!
//! O algoritmo:
//! 1. A imagem é lida já reduzida para 64x64, sem manter a proporção (o new_from_file_at_scale do GdkPixbuf):
//!    cada parte da imagem continua com a mesma fração dos pixels. O alfa é ignorado.
//! 2. Cada pixel vira Lab e LCh (as contas do papirus.rs). Só vota o pixel com croma >= GREY_CHROMA (12) e
//!    claridade entre 15 e 92: cinza, quase preto e quase branco não dizem a cor. O voto pesa o croma.
//! 3. Menos de 3% dos pixels votando: a imagem não tem cor (None).
//! 4. Os votos vão para 36 faixas de matiz de 10 graus (floor(h / 10)). A faixa vencedora é a de maior soma com
//!    as duas vizinhas (circular); no empate, a de menor índice.
//! 5. A média de L, a e b dos pixels que votaram nessas três faixas, pesada pelo croma, vira LCh, e a resposta é
//!    a cor do Papirus mais perto dela pelo distance do papirus.rs, entre todas menos black, grey e white; no
//!    empate, a primeira da tabela. As somas seguem a ordem dos pixels, linha a linha.
use std::path::{Path, PathBuf};

use swaydesk_common::config::{home, PAPIRUS_COLORS};

use crate::papirus::{distance_lch, lab_linear, lch, lch_of_lab, GREY_CHROMA};

/// O lado da imagem reduzida
pub const SIZE: i32 = 64;
const MIN_LIGHTNESS: f64 = 15.0;
const MAX_LIGHTNESS: f64 = 92.0;
const BINS: usize = 36;
/// Abaixo desta porcentagem de pixels com cor, a imagem é cinza
const MIN_COLORED_PERCENT: usize = 3;
/// As cores do Papirus que não são escolhidas pelo matiz
const NEUTRALS: &[&str] = &["black", "grey", "white"];
/// A versão do algoritmo no estado: outra versão calcula de novo
const STATE_VERSION: &str = "1";

/// O papel de parede em uso (um link para a imagem do dia ou a escolhida). Sempre na HOME, como quem o grava
/// e mostra (o bing-wallpaper, o `output * bg` do config do Sway, os bloqueios e o ensure_wallpaper): com um
/// XDG_DATA_HOME próprio, o data_home apontava para outra pasta, e a cor automática não achava a imagem.
pub fn wallpaper() -> PathBuf {
    home().join(crate::desktop::WALLPAPER)
}

fn linear(channel: u8) -> f64 {
    let v = channel as f64 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// O nome da cor do Papirus para os pixels (RGB ou RGBA, `channels` bytes por pixel, `rowstride` bytes por
/// linha). None: a imagem não tem cor (ou não tem pixels).
pub fn pick_color(pixels: &[u8], width: usize, height: usize, channels: usize, rowstride: usize) -> Option<&'static str> {
    // Os canais em 0..=255 já sem a curva: a tabela evita 3 powf por pixel
    let table: Vec<f64> = (0..=255u8).map(linear).collect();
    let mut votes: Vec<(f64, f64, f64, f64, usize)> = Vec::new(); // L, a, b, croma, faixa
    let mut total = 0usize;
    for y in 0..height {
        for x in 0..width {
            let at = y * rowstride + x * channels;
            let Some(rgb) = pixels.get(at..at + 3) else { continue };
            total += 1;
            let lab = lab_linear([table[rgb[0] as usize], table[rgb[1] as usize], table[rgb[2] as usize]]);
            let (lightness, chroma, hue) = lch_of_lab(lab);
            if chroma >= GREY_CHROMA && (MIN_LIGHTNESS..=MAX_LIGHTNESS).contains(&lightness) {
                votes.push((lab.0, lab.1, lab.2, chroma, (hue / 10.0).floor() as usize % BINS));
            }
        }
    }
    if total == 0 || votes.len() * 100 < MIN_COLORED_PERCENT * total {
        return None;
    }
    let mut histogram = [0.0f64; BINS];
    for vote in &votes {
        histogram[vote.4] += vote.3;
    }
    let near = |bin: usize, other: usize| (bin + BINS - other) % BINS <= 1 || (other + BINS - bin) % BINS <= 1;
    let score = |bin: usize| histogram[(bin + BINS - 1) % BINS] + histogram[bin] + histogram[(bin + 1) % BINS];
    let mut peak = 0;
    for bin in 1..BINS {
        if score(bin) > score(peak) {
            peak = bin;
        }
    }
    let (mut weight, mut sum_l, mut sum_a, mut sum_b) = (0.0, 0.0, 0.0, 0.0);
    for &(l, a, b, chroma, bin) in &votes {
        if near(bin, peak) {
            weight += chroma;
            sum_l += chroma * l;
            sum_a += chroma * a;
            sum_b += chroma * b;
        }
    }
    let mean = lch_of_lab((sum_l / weight, sum_a / weight, sum_b / weight));
    let mut best: Option<(&'static str, f64)> = None;
    for (name, hex) in PAPIRUS_COLORS.iter().filter(|(name, _)| !NEUTRALS.contains(name)) {
        let d = distance_lch(mean, lch(hex));
        if best.is_none_or(|(_, b)| d < b) {
            best = Some((name, d));
        }
    }
    best.map(|(name, _)| name)
}

/// A cor do papel de parede, lido pelo GdkPixbuf já reduzido. None se ele não existe ou não abre; Some(None) se
/// não tem cor.
pub fn wallpaper_color(path: &Path) -> Option<Option<&'static str>> {
    let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(path, SIZE, SIZE, false).ok()?;
    let bytes = pixbuf.read_pixel_bytes();
    Some(pick_color(
        &bytes,
        pixbuf.width() as usize,
        pixbuf.height() as usize,
        pixbuf.n_channels() as usize,
        pixbuf.rowstride() as usize,
    ))
}

/// O estado do último papel calculado: "<caminho real>\n<mtime em ns>\n<cor ou none>\n<versão>\n" (o mesmo do
/// Python).
fn state_file() -> PathBuf {
    crate::themes::state_dir().join("accent-auto")
}

/// O caminho real e o mtime em nanossegundos do papel de parede.
fn identity(path: &Path) -> Option<(PathBuf, u128)> {
    let real = std::fs::canonicalize(path).ok()?;
    let modified = std::fs::metadata(&real).ok()?.modified().ok()?;
    let ns = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    Some((real, ns))
}

/// A cor guardada para este papel de parede (Some(None): sem cor), se o estado é dele, da versão atual e a cor é
/// do Papirus.
fn cached(state: &str, real: &Path, ns: u128) -> Option<Option<&'static str>> {
    let mut lines = state.lines();
    let (path, mtime, name, version) = (lines.next()?, lines.next()?, lines.next()?, lines.next()?);
    if path != real.to_string_lossy() || mtime.parse::<u128>().ok()? != ns || version != STATE_VERSION {
        return None;
    }
    if name == "none" {
        return Some(None);
    }
    PAPIRUS_COLORS.iter().find(|(n, _)| *n == name).map(|(n, _)| Some(*n))
}

/// A cor do papel de parede em uso, pelo estado quando o arquivo é o mesmo (sem abrir a imagem). Calculada,
/// grava o estado. None: o papel não existe ou não abre; Some(None): não tem cor.
pub fn current_color() -> Option<Option<&'static str>> {
    let (real, ns) = identity(&wallpaper())?;
    let file = state_file();
    if let Some(color) = std::fs::read_to_string(&file).ok().and_then(|state| cached(&state, &real, ns)) {
        return Some(color);
    }
    let color = wallpaper_color(&real)?;
    // Sem o estado, só se calcula de novo da próxima vez
    let state = format!("{}\n{ns}\n{}\n{STATE_VERSION}\n", real.to_string_lossy(), color.unwrap_or("none"));
    let _ = crate::desktop::write_file(&file, &state);
    Some(color)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_wallpaper_is_the_one_the_session_writes_and_shows() {
        // O caminho do ensure_wallpaper (desktop.rs) e do bloqueio (lock-logic), na HOME, não no XDG_DATA_HOME
        assert_eq!(wallpaper(), home().join(".local/share/wallpaper/current.jpg"));
    }

    /// Uma imagem 64x64 RGB a partir de faixas horizontais: (cor, linhas). As linhas somam 64.
    pub(crate) fn stripes(parts: &[(&str, usize)]) -> Vec<u8> {
        let mut pixels = Vec::new();
        for (hex, rows) in parts {
            let rgb: Vec<u8> = (0..3).map(|i| u8::from_str_radix(&hex[1 + 2 * i..3 + 2 * i], 16).unwrap()).collect();
            for _ in 0..rows * 64 {
                pixels.extend_from_slice(&rgb);
            }
        }
        assert_eq!(pixels.len(), 64 * 64 * 3);
        pixels
    }

    fn pick(parts: &[(&str, usize)]) -> Option<&'static str> {
        pick_color(&stripes(parts), 64, 64, 3, 64 * 3)
    }

    /// Os vetores comuns com o sway_wallpaper_accent.py: (faixas, cor esperada; None: sem cor).
    pub(crate) const VECTORS: &[(&[(&str, usize)], Option<&str>)] = &[
        (&[("#1a5fb4", 64)], Some("adwaita")),
        (&[("#26a269", 64)], Some("teal")),
        (&[("#c01c28", 64)], Some("red")),
        (&[("#e66100", 64)], Some("yaru")),
        (&[("#f6d32d", 64)], Some("yellow")),
        (&[("#613583", 64)], Some("violet")),
        (&[("#808080", 32), ("#e25252", 32)], Some("red")),
        (&[("#808080", 64)], None),
        (&[("#000000", 64)], None),
        (&[("#ffffff", 64)], None),
        (&[("#e25252", 1), ("#808080", 63)], None),
        (&[("#e25252", 3), ("#808080", 61)], Some("red")),
        (&[("#3584e4", 20), ("#ee923a", 44)], Some("orange")),
        (&[("#3584e4", 44), ("#ee923a", 20)], Some("adwaita")),
        (&[("#300000", 64)], None),
        (&[("#f8e8e8", 64)], None),
        (&[("#607d8b", 64)], Some("bluegrey")),
        (&[("#2e3440", 40), ("#88c0d0", 24)], Some("cyan")),
        (&[("#1c1c1c", 64)], None),
        (&[("#a30002", 64)], Some("carmine")),
    ];

    #[test]
    fn every_chromatic_papirus_color_picks_itself() {
        // O palebrown (croma 11,4) é um cinza para o voto: não sai de imagem nenhuma, como as neutras
        for (name, hex) in PAPIRUS_COLORS {
            let expected = (!NEUTRALS.contains(name) && *name != "palebrown").then_some(*name);
            assert_eq!(pick(&[(hex, 64)]), expected, "{hex}");
        }
    }

    #[test]
    fn shared_vectors() {
        for (parts, expected) in VECTORS {
            assert_eq!(pick(parts), *expected, "{parts:?}");
        }
    }

    #[test]
    fn state_only_for_the_same_file() {
        let real = Path::new("/home/u/.local/share/wallpaper/bing-2026-11-20.jpg");
        let state = "/home/u/.local/share/wallpaper/bing-2026-11-20.jpg\n1763600000123456789\nteal\n1\n";
        assert_eq!(cached(state, real, 1763600000123456789), Some(Some("teal")));
        assert_eq!(cached(&state.replace("teal", "none"), real, 1763600000123456789), Some(None));
        assert_eq!(cached(&state.replace("\n1\n", "\n2\n"), real, 1763600000123456789), None);
        assert_eq!(cached(&state.replace("\n1\n", "\n"), real, 1763600000123456789), None);
        assert_eq!(cached(state, real, 1763600000123456788), None);
        assert_eq!(cached(state, Path::new("/outro.jpg"), 1763600000123456789), None);
        assert_eq!(cached(&state.replace("teal", "roxo"), real, 1763600000123456789), None);
        assert_eq!(cached("", real, 1), None);
    }

    #[test]
    fn rgba_and_padded_rows() {
        // 2x2 com alfa e 4 bytes de sobra por linha: o alfa e a sobra não contam
        let row = [0xe2, 0x52, 0x52, 0x00, 0xe2, 0x52, 0x52, 0x10, 9, 9, 9, 9];
        let pixels = [row, row].concat();
        assert_eq!(pick_color(&pixels, 2, 2, 4, 12), Some("red"));
        assert_eq!(pick_color(&[], 0, 0, 3, 0), None);
    }

    #[test]
    fn reads_a_file() {
        let dir = std::env::temp_dir().join(format!("swaydesk-wallaccent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Um PPM 128x96 verde: o GdkPixbuf reduz e a cor fica
        let file = dir.join("green.ppm");
        let mut data = b"P6\n128 96\n255\n".to_vec();
        for _ in 0..128 * 96 {
            data.extend_from_slice(&[0x87, 0xb1, 0x58]);
        }
        std::fs::write(&file, data).unwrap();
        assert_eq!(wallpaper_color(&file), Some(Some("green")));
        assert_eq!(wallpaper_color(&dir.join("missing.jpg")), None);
        std::fs::write(dir.join("broken.jpg"), b"not an image").unwrap();
        assert_eq!(wallpaper_color(&dir.join("broken.jpg")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
