//! O que a grade precisa da dock (o porte das funções de `sway_dock.py` que o `sway-drawer` usa).
//!
//! Só a dock grava os fixados dela: a grade lê a lista que a dock publica em
//! `$XDG_RUNTIME_DIR/sway-dock.pinned-ids` e pede para fixar ou desafixar por `sway-dock pin|unpin`.
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Os pedidos que o `sway-dock` aceita.
pub const PIN_ACTIONS: &[&str] = &["pin", "unpin"];

/// As extensões de imagem de um ícone dado por caminho.
pub const ICON_EXTENSIONS: &[&str] = &[".png", ".svg", ".xpm"];

pub fn pinned_ids_path(run: &Path) -> PathBuf {
    run.join("sway-dock.pinned-ids")
}

/// Um retângulo (x, y, largura, altura).
pub type Rect = (i64, i64, i64, i64);

/// A área da dock à vista, em coordenadas do layout do Sway, que a dock publica. Com a grade aberta, ela
/// fica fora da área de cliques da grade: um clique na dock chega à dock.
pub fn area_path(run: &Path) -> PathBuf {
    run.join("sway-dock.area")
}

/// Existe enquanto a grade está à vista: a dock só pede para escondê-la (SIGUSR2) se ele existir. Uma
/// grade antiga, sem o pedido, morreria com o sinal.
pub fn shown_path(run: &Path) -> PathBuf {
    run.join("sway-drawer.shown")
}

/// As áreas publicadas pela dock. Sem o arquivo, ou com uma linha estranha, a linha não conta.
pub fn read_area(run: &Path) -> Vec<Rect> {
    let Ok(text) = fs::read_to_string(area_path(run)) else { return vec![] };
    text.lines()
        .filter_map(|line| {
            let n: Vec<i64> = line.split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?;
            match n[..] {
                [x, y, w, h] if w > 0 && h > 0 => Some((x, y, w, h)),
                _ => None,
            }
        })
        .collect()
}

/// As partes das áreas que caem no monitor, em coordenadas da janela da grade, que o cobre inteiro.
pub fn holes(monitor: Rect, area: &[Rect]) -> Vec<Rect> {
    let (mx, my, mw, mh) = monitor;
    area.iter()
        .filter_map(|&(x, y, w, h)| {
            let (left, top) = (x.max(mx), y.max(my));
            let (right, bottom) = ((x + w).min(mx + mw), (y + h).min(my + mh));
            (right > left && bottom > top).then(|| (left - mx, top - my, right - left, bottom - top))
        })
        .collect()
}

/// Quando a dock publicou a lista pela última vez; None sem o arquivo.
pub fn pinned_ids_mtime(run: &Path) -> Option<SystemTime> {
    fs::metadata(pinned_ids_path(run)).and_then(|m| m.modified()).ok()
}

/// O conjunto publicado pela dock; None sem o arquivo. Outro erro sobe.
pub fn read_pinned_ids(run: &Path) -> io::Result<Option<HashSet<String>>> {
    match fs::read_to_string(pinned_ids_path(run)) {
        Ok(text) => Ok(Some(text.lines().filter(|l| !l.is_empty()).map(str::to_string).collect())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Se o app está entre os fixados, sem diferenciar maiúsculas: o id que a dock resolveu pode vir de
/// outro .desktop do mesmo app.
pub fn is_pinned(ids: &HashSet<String>, app_id: &str) -> bool {
    let wanted = app_id.to_lowercase();
    ids.iter().any(|id| id.to_lowercase() == wanted)
}

/// Caminhos a tentar para um ícone absoluto: o próprio caminho e, se ele não termina numa extensão
/// de imagem, com cada uma. Alguns .desktop omitem a extensão, e os ícones de AppImage às vezes não a
/// têm nem no arquivo. Um ponto no nome não é extensão: org.example.App, Bruno-1.2.
pub fn icon_paths(spec: &str) -> Vec<String> {
    let lower = spec.to_lowercase();
    if ICON_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return vec![spec.to_string()];
    }
    std::iter::once(spec.to_string()).chain(ICON_EXTENSIONS.iter().map(|ext| format!("{spec}{ext}"))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_paths_try_the_extensions_only_without_one() {
        assert_eq!(icon_paths("/i/app.PNG"), ["/i/app.PNG"]);
        assert_eq!(icon_paths("/i/org.example.App"),
                   ["/i/org.example.App", "/i/org.example.App.png", "/i/org.example.App.svg", "/i/org.example.App.xpm"]);
    }

    #[test]
    fn the_dock_area_becomes_holes_in_the_grid() {
        let dir = std::env::temp_dir().join(format!("swaydesk-dock-area-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(read_area(&dir).is_empty());
        // Duas docks, uma linha estranha e uma vazia de tamanho: só as válidas contam
        fs::write(area_path(&dir), "574 650 132 70\nlixo\n1 2 0 5\n1854 1010 132 70\n").unwrap();
        let area = read_area(&dir);
        assert_eq!(area, [(574, 650, 132, 70), (1854, 1010, 132, 70)]);
        // A grade no primeiro monitor (1280x720 na origem) só vê a dock dele, em coordenadas da janela
        assert_eq!(holes((0, 0, 1280, 720), &area), [(574, 650, 132, 70)]);
        // No segundo, à direita (1920x1080 em x=1280): a outra, deslocada
        assert_eq!(holes((1280, 0, 1920, 1080), &area), [(574, 1010, 132, 70)]);
        // Uma dock que passa da borda é cortada nela
        assert_eq!(holes((0, 0, 600, 700), &[(574, 650, 132, 70)]), [(574, 650, 26, 50)]);
        assert_eq!(shown_path(&dir), dir.join("sway-drawer.shown"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pinned_ids_are_read_and_compared_ignoring_case() {
        let dir = std::env::temp_dir().join(format!("swaydesk-dock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(read_pinned_ids(&dir).unwrap().is_none());
        assert!(pinned_ids_mtime(&dir).is_none());
        fs::write(pinned_ids_path(&dir), "firefox\n\nOrg.Gnome.Nautilus\n").unwrap();
        let ids = read_pinned_ids(&dir).unwrap().unwrap();
        assert!(is_pinned(&ids, "org.gnome.Nautilus"));
        assert!(!is_pinned(&ids, "kitty"));
        assert!(pinned_ids_mtime(&dir).is_some());
        let _ = fs::remove_dir_all(&dir);
    }
}
