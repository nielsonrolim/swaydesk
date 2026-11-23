//! O fundo desfocado do monitor, pelo GdkPixbuf: o `make_blur` do `sway_lock.py`, no mesmo arquivo do
//! cache. Só depois do locked, fora da linha principal: no caminho do bloqueio, só o cache.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gtk4::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};
use swaydesk_lock_logic as sl;

/// Gera o fundo, se faltar, e fica com os `keep` mais novos. O caminho, ou None sem papel de parede.
pub fn make_blur(wallpaper: &Path, width: i32, height: i32, env: &HashMap<String, String>, keep: usize) -> Option<PathBuf> {
    let path = sl::blur_path(wallpaper, width, height, env)?;
    if path.is_file() {
        return Some(path);
    }
    let source = Pixbuf::from_file(wallpaper).ok()?;
    let (cw, ch) = sl::cover_size(source.width(), source.height(), width, height);
    let scaled = source.scale_simple(cw, ch, InterpType::Bilinear)?;
    let mut image = scaled.new_subpixbuf((cw - width) / 2, (ch - height) / 2, width, height).copy()?;
    let (small_w, small_h) = ((width / sl::BLUR_FACTOR).max(1), (height / sl::BLUR_FACTOR).max(1));
    for _ in 0..2 {
        image = image.scale_simple(small_w, small_h, InterpType::Bilinear)?;
        image = image.scale_simple(width, height, InterpType::Bilinear)?;
    }
    if image.has_alpha() {
        image = image.composite_color_simple(width, height, InterpType::Nearest, 255, 1, 0, 0)?;
    }
    let dark = Pixbuf::new(Colorspace::Rgb, true, 8, width, height)?;
    dark.fill(sl::DARKEN); // 0x000000 com alfa DARKEN
    let with_alpha = image.add_alpha(false, 0, 0, 0).ok()?;
    dark.composite(&with_alpha, 0, 0, width, height, 0.0, 0.0, 1.0, 1.0, InterpType::Nearest, 255);
    std::fs::create_dir_all(path.parent()?).ok()?;
    let tmp = path.with_file_name(format!(".{}.{}", path.file_name()?.to_string_lossy(), std::process::id()));
    with_alpha.savev(&tmp, "png", &[]).ok()?;
    std::fs::rename(&tmp, &path).ok()?;
    sl::prune(path.parent()?, keep, Some(&path));
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(name: &str) -> (PathBuf, HashMap<String, String>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("swaydesk-lock-blur-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let env: HashMap<String, String> = [("XDG_CACHE_HOME".to_string(), dir.join("cache").display().to_string())].into();
        let image = dir.join("dia.png");
        let pixbuf = Pixbuf::new(Colorspace::Rgb, false, 8, 400, 300).unwrap();
        pixbuf.fill(0xFF8000FF);
        pixbuf.savev(&image, "png", &[]).unwrap();
        (dir, env, image)
    }

    #[test]
    fn makes_a_darker_image_of_the_monitor_size_once() {
        let (_dir, env, image) = setup("once");
        let path = make_blur(&image, 320, 200, &env, 4).unwrap();
        assert_eq!(Some(path.clone()), sl::cached_blur(&image, 320, 200, &env));
        let out = Pixbuf::from_file(&path).unwrap();
        assert_eq!((out.width(), out.height()), (320, 200));
        let red = out.read_pixel_bytes()[0];
        assert!(red < 0xFF && red > 0x40, "{red}"); // o laranja escurecido
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(make_blur(&image, 320, 200, &env, 4), Some(path.clone()));
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    }

    #[test]
    fn keeps_only_the_newest() {
        let (dir, env, image) = setup("keep");
        for width in 100..106 {
            make_blur(&image, width, 100, &env, 3).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let names: Vec<String> = std::fs::read_dir(dir.join("cache/swaydesk")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names.iter().filter(|n| n.starts_with("lock-")).count(), 3);
        assert!(!names.iter().any(|n| n.starts_with(".lock-")));
    }

    #[test]
    fn without_wallpaper_nothing() {
        let (dir, env, _image) = setup("none");
        assert_eq!(make_blur(&dir.join("nada.jpg"), 100, 100, &env, 4), None);
    }
}
