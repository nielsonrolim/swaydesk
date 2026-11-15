//! O teclado do sistema: o `/etc/default/keyboard` (Debian, Ubuntu), senão o do systemd (Arch). O primeiro
//! login da sessão do pacote o grava no `50-local.conf`, como o `keyboard_block` do install.sh, e a tela de
//! login o aplica no Sway dela.
use std::path::{Path, PathBuf};

/// Os arquivos do systemd, na ordem em que o localectl os usa.
const XORG: &str = "/etc/X11/xorg.conf.d/00-keyboard.conf";
const VCONSOLE: &str = "/etc/vconsole.conf";
const KBD_MODEL_MAP: &str = "/usr/share/systemd/kbd-model-map";

/// O bloco do Sway com o teclado do sistema, ou vazio (sem o arquivo, sem layout, ou só "us" sem mais nada).
/// O mesmo texto do `keyboard_block` do install.sh.
pub fn keyboard_block(file: &Path) -> String {
    match std::fs::read_to_string(file) {
        Ok(text) => keyboard_block_text(file, &text),
        Err(_) => String::new(),
    }
}

/// O teclado do sistema no formato do /etc/default/keyboard: esse arquivo, se existir; senão, o do systemd.
/// Devolve o arquivo de onde veio e o texto.
pub fn system_keyboard(default: &Path) -> Option<(PathBuf, String)> {
    if default.exists() {
        return std::fs::read_to_string(default).ok().map(|text| (default.to_path_buf(), text));
    }
    systemd_keyboard(Path::new(XORG), Path::new(VCONSOLE), Path::new(KBD_MODEL_MAP))
}

/// O teclado pelo systemd, no formato do /etc/default/keyboard, para as distros sem esse arquivo (Arch). O
/// layout X11 do localectl (`Option "XkbLayout"` no 00-keyboard.conf) vence; sem ele, o KEYMAP do
/// vconsole.conf vira o layout X11 pela tabela do systemd (kbd-model-map), como o localectl faz. Devolve o
/// arquivo de onde veio e o texto.
pub fn systemd_keyboard(xorg: &Path, vconsole: &Path, map: &Path) -> Option<(PathBuf, String)> {
    if let Ok(text) = std::fs::read_to_string(xorg) {
        let mut out = String::new();
        for line in text.lines() {
            let mut words = line.split('"').map(str::trim);
            if words.next() != Some("Option") {
                continue;
            }
            let (Some(key), _, Some(value)) = (words.next(), words.next(), words.next()) else { continue };
            if let Some(name) = key.strip_prefix("Xkb") {
                out.push_str(&format!("XKB{}=\"{value}\"\n", name.to_ascii_uppercase()));
            }
        }
        if out.contains("XKBLAYOUT=") {
            return Some((xorg.to_path_buf(), out));
        }
    }
    let text = std::fs::read_to_string(vconsole).ok()?;
    let keymap = text.lines().find_map(|l| l.strip_prefix("KEYMAP="))?.trim().trim_matches('"').to_string();
    let table = std::fs::read_to_string(map).ok()?;
    // As colunas: o mapa do console, o layout, o modelo, a variante e as opções do X11 ("-" é vazio)
    let row = table.lines().map(|l| l.split_whitespace().collect::<Vec<_>>()).find(|r| r.len() >= 5 && r[0] == keymap)?;
    let field = |i: usize| if row[i] == "-" { "" } else { row[i] };
    let out = format!("XKBMODEL=\"{}\"\nXKBLAYOUT=\"{}\"\nXKBVARIANT=\"{}\"\n", field(2), field(1), field(3));
    Some((vconsole.to_path_buf(), out))
}

/// As opções xkb do Sway (model, layout, variant, options), na ordem, só as não vazias, de um texto no
/// formato do /etc/default/keyboard. Vazio quando não muda nada: sem layout, ou só "us" sem variante nem
/// opções. Um valor com caractere fora de `[A-Za-z0-9_,:()+-]` fica de fora, como no install.sh: ele vai
/// para a config do Sway e para o swaymsg.
pub fn xkb_settings(text: &str) -> Vec<(&'static str, String)> {
    let mut values: [(&'static str, Option<String>); 4] = [("model", None), ("layout", None), ("variant", None), ("options", None)];
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let Some(name) = key.strip_prefix("XKB") else { continue };
        let value = value.strip_prefix('"').unwrap_or(value);
        let value = value.strip_suffix('"').unwrap_or(value);
        if !value.chars().all(|c| c.is_ascii_alphanumeric() || "_,:()+-".contains(c)) {
            continue;
        }
        if let Some(slot) = values.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
            slot.1 = Some(value.to_string());
        }
    }
    let get = |i: usize| values[i].1.clone().unwrap_or_default();
    let layout = get(1);
    if layout.is_empty() || (layout == "us" && get(2).is_empty() && get(3).is_empty()) {
        return Vec::new();
    }
    values.into_iter().filter_map(|(k, v)| v.filter(|v| !v.is_empty()).map(|v| (k, v))).collect()
}

/// O `keyboard_block` de um texto no formato do /etc/default/keyboard; `file` só vai para o comentário.
pub fn keyboard_block_text(file: &Path, text: &str) -> String {
    let settings = xkb_settings(text);
    if settings.is_empty() {
        return String::new();
    }
    let mut block = format!("\n# --- Teclado do sistema ({}), lido na instalação ---\ninput type:keyboard {{\n", file.display());
    for (key, value) in &settings {
        block.push_str(&format!("    xkb_{key:<8} {value}\n"));
    }
    block.push_str("}\n");
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swaydesk-keyboard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn keyboard_like_install_sh() {
        let dir = tempdir("kbd");
        let file = dir.join("keyboard");
        std::fs::write(&file, "XKBMODEL=\"pc105\"\nXKBLAYOUT=\"br,us\"\nXKBVARIANT=\"abnt2,intl\"\nXKBOPTIONS=\"\"\nBACKSPACE=\"guess\"\n").unwrap();
        assert_eq!(
            keyboard_block(&file),
            format!(
                "\n# --- Teclado do sistema ({}), lido na instalação ---\ninput type:keyboard {{\n    xkb_model    pc105\n    xkb_layout   br,us\n    xkb_variant  abnt2,intl\n}}\n",
                file.display()
            )
        );
        std::fs::write(&file, "XKBLAYOUT=\"us\"\nXKBVARIANT=\"\"\n").unwrap();
        assert_eq!(keyboard_block(&file), "");
        std::fs::write(&file, "XKBLAYOUT=\"us\"\nXKBOPTIONS=\"ctrl:nocaps\"\n").unwrap();
        assert!(keyboard_block(&file).contains("xkb_options  ctrl:nocaps"));
        // Um valor estranho fica de fora, como no install.sh
        std::fs::write(&file, "XKBLAYOUT=\"br; exec x\"\n").unwrap();
        assert_eq!(keyboard_block(&file), "");
        assert_eq!(keyboard_block(&dir.join("nao-existe")), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keyboard_from_systemd() {
        let dir = tempdir("systemd-kbd");
        let (xorg, vconsole, map) = (dir.join("00-keyboard.conf"), dir.join("vconsole.conf"), dir.join("kbd-model-map"));
        std::fs::write(&map, "# comentário\nus\t\tus\tpc105\t\t-\tterminate:ctrl_alt_bksp\nbr-abnt2\t\tbr\tabnt2\t\t-\tterminate:ctrl_alt_bksp\nbr-latin1-us\tbr\tpc105\tnodeadkeys\tterminate:ctrl_alt_bksp\n").unwrap();
        assert_eq!(systemd_keyboard(&xorg, &vconsole, &map), None);
        // Só o console (o Arch logo depois do archinstall): br-abnt2 vira br, modelo abnt2, sem variante
        std::fs::write(&vconsole, "KEYMAP=br-abnt2\nFONT=ter-v16n\n").unwrap();
        let (source, text) = systemd_keyboard(&xorg, &vconsole, &map).unwrap();
        assert_eq!(source, vconsole);
        assert_eq!(text, "XKBMODEL=\"abnt2\"\nXKBLAYOUT=\"br\"\nXKBVARIANT=\"\"\n");
        assert!(keyboard_block_text(&source, &text).contains("    xkb_model    abnt2\n    xkb_layout   br\n}"));
        std::fs::write(&vconsole, "KEYMAP=br-latin1-us\n").unwrap();
        assert!(systemd_keyboard(&xorg, &vconsole, &map).unwrap().1.contains("XKBVARIANT=\"nodeadkeys\""));
        // Um mapa fora da tabela: nada
        std::fs::write(&vconsole, "KEYMAP=xx\n").unwrap();
        assert_eq!(systemd_keyboard(&xorg, &vconsole, &map), None);
        // O layout X11 do localectl vence o console
        std::fs::write(&xorg, "Section \"InputClass\"\n        Identifier \"system-keyboard\"\n        MatchIsKeyboard \"on\"\n        Option \"XkbLayout\" \"us,br\"\n        Option \"XkbVariant\" \"intl,abnt2\"\nEndSection\n").unwrap();
        let (source, text) = systemd_keyboard(&xorg, &vconsole, &map).unwrap();
        assert_eq!(source, xorg);
        assert_eq!(text, "XKBLAYOUT=\"us,br\"\nXKBVARIANT=\"intl,abnt2\"\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_for_swaymsg() {
        assert_eq!(
            xkb_settings("XKBMODEL=\"pc105\"\nXKBLAYOUT=\"br\"\nXKBVARIANT=\"\"\nXKBOPTIONS=\"ctrl:nocaps\"\n"),
            vec![("model", "pc105".to_string()), ("layout", "br".to_string()), ("options", "ctrl:nocaps".to_string())]
        );
        assert!(xkb_settings("XKBLAYOUT=\"us\"\n").is_empty());
        assert!(xkb_settings("XKBLAYOUT=\"br; exec x\"\n").is_empty());
    }

    #[test]
    fn the_default_file_wins_when_it_exists() {
        let dir = tempdir("system");
        let file = dir.join("keyboard");
        std::fs::write(&file, "XKBLAYOUT=\"br\"\n").unwrap();
        assert_eq!(system_keyboard(&file), Some((file.clone(), "XKBLAYOUT=\"br\"\n".to_string())));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
