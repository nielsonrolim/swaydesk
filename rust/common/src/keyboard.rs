//! O teclado do sistema: o `/etc/default/keyboard` (Debian, Ubuntu), senão o do systemd (Arch). A cada
//! login, a sessão do pacote o grava no `50-local.conf`, como o `keyboard_block` do install.sh, enquanto
//! ninguém mexer no bloco; a tela de login o aplica no Sway dela.
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

/// O teclado do sistema no formato do /etc/default/keyboard: o do arquivo que mudou por último entre o
/// /etc/default/keyboard, o 00-keyboard.conf e o vconsole.conf. O localectl grava em lugares diferentes: no
/// Debian, o /etc/default/keyboard; no Arch, o 00-keyboard.conf (e o vconsole.conf); no Ubuntu 26.04, só o
/// vconsole.conf, com as chaves XKB*. O /etc/default/keyboard que o instalador deixa no Arch fica parado com o
/// teclado da instalação. Empate: o /etc/default/keyboard, depois o 00-keyboard.conf. Um vconsole.conf só com o
/// KEYMAP (sem XKB*) não entra na disputa: a tradução pelo kbd-model-map perde layouts e opções, e ele só vale
/// sem nenhum dos outros, como antes. Devolve o arquivo de onde veio e o texto.
pub fn system_keyboard(default: &Path) -> Option<(PathBuf, String)> {
    system_keyboard_from(default, Path::new(XORG), Path::new(VCONSOLE), Path::new(KBD_MODEL_MAP))
}

/// O `system_keyboard` com os arquivos do systemd à escolha (os testes).
pub fn system_keyboard_from(default: &Path, xorg: &Path, vconsole: &Path, map: &Path) -> Option<(PathBuf, String)> {
    let from_default = std::fs::read_to_string(default).ok().map(|text| (default.to_path_buf(), text));
    let (xkb, keymap) = vconsole_keyboard(vconsole, map);
    newest([from_default, xorg_keyboard(xorg), xkb]).or(keymap)
}

/// O teclado pelo systemd, no formato do /etc/default/keyboard, para as distros sem esse arquivo (Arch): o
/// 00-keyboard.conf ou as chaves XKB* do vconsole.conf, o que mudou por último (empate: o 00-keyboard.conf);
/// sem nenhum dos dois, o KEYMAP do vconsole.conf traduzido. Devolve o arquivo de onde veio e o texto.
pub fn systemd_keyboard(xorg: &Path, vconsole: &Path, map: &Path) -> Option<(PathBuf, String)> {
    let (xkb, keymap) = vconsole_keyboard(vconsole, map);
    newest([xorg_keyboard(xorg), xkb]).or(keymap)
}

/// O de data de modificação mais nova; no empate (ou sem data), o que vem antes.
fn newest<const N: usize>(sources: [Option<(PathBuf, String)>; N]) -> Option<(PathBuf, String)> {
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    sources.into_iter().flatten().fold(None, |best: Option<(PathBuf, String)>, next| match best {
        Some(b) if modified(&next.0) <= modified(&b.0) => Some(b),
        _ => Some(next),
    })
}

/// O layout X11 do localectl no 00-keyboard.conf (`Option "XkbLayout"`), se houver.
fn xorg_keyboard(xorg: &Path) -> Option<(PathBuf, String)> {
    let text = std::fs::read_to_string(xorg).ok()?;
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
    out.contains("XKBLAYOUT=").then(|| (xorg.to_path_buf(), out))
}

/// O vconsole.conf: as chaves XKB* que o localectl novo grava nele (o Ubuntu 26.04), no primeiro; senão, no
/// segundo, o KEYMAP traduzido para o layout X11 pela tabela do systemd (kbd-model-map), como o localectl faz.
fn vconsole_keyboard(vconsole: &Path, map: &Path) -> (Option<(PathBuf, String)>, Option<(PathBuf, String)>) {
    let Ok(text) = std::fs::read_to_string(vconsole) else { return (None, None) };
    let xkb: String = text
        .lines()
        .filter(|l| ["XKBMODEL=", "XKBLAYOUT=", "XKBVARIANT=", "XKBOPTIONS="].iter().any(|k| l.trim_start().starts_with(k)))
        .map(|l| format!("{}\n", l.trim()))
        .collect();
    if xkb.contains("XKBLAYOUT=") {
        return (Some((vconsole.to_path_buf(), xkb)), None);
    }
    (None, keymap_keyboard(vconsole, &text, map))
}

/// O KEYMAP do vconsole.conf pela tabela do kbd-model-map.
fn keymap_keyboard(vconsole: &Path, text: &str, map: &Path) -> Option<(PathBuf, String)> {
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

/// A tabela de compose do US internacional: a do locale (`include "%L"`), com ' + c dando ç em vez do ć do
/// en_US. O primeiro login a grava no `~/.XCompose`, e a tela de login a usa pelo GTK.
pub const INTL_COMPOSE: &str = include_str!("intl.XCompose");

/// Se o teclado (no formato do /etc/default/keyboard) tem o US internacional em algum dos layouts: `us` com a
/// variante `intl` ou `altgr-intl` na mesma posição (`br,us` com `abnt2,intl` conta).
pub fn is_us_intl(text: &str) -> bool {
    let settings = xkb_settings(text);
    let get = |key: &str| settings.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str()).unwrap_or("");
    let variants: Vec<&str> = get("variant").split(',').collect();
    get("layout").split(',').enumerate().any(|(i, layout)| {
        layout == "us" && matches!(variants.get(i).copied(), Some("intl" | "altgr-intl"))
    })
}

/// O começo e o fim do cabeçalho do bloco do teclado do sistema no `50-local.conf`.
const BLOCK_HEAD: &str = "\n# --- Teclado do sistema (";
const BLOCK_HEAD_END: &str = "), lido na instalação ---\n";

/// O `keyboard_block` de um texto no formato do /etc/default/keyboard; `file` só vai para o comentário.
pub fn keyboard_block_text(file: &Path, text: &str) -> String {
    let settings = xkb_settings(text);
    if settings.is_empty() {
        return String::new();
    }
    let mut block = format!("{BLOCK_HEAD}{}{BLOCK_HEAD_END}input type:keyboard {{\n", file.display());
    for (key, value) in &settings {
        block.push_str(&format!("    xkb_{key:<8} {value}\n"));
    }
    block.push_str("}\n");
    block
}

/// Onde está o bloco do teclado do sistema num `50-local.conf`: do `\n` antes do cabeçalho até o `}\n` que o
/// fecha, o mesmo texto que o `keyboard_block_text` gera. None: sem bloco (ou sem o fim dele).
pub fn find_block(conf: &str) -> Option<std::ops::Range<usize>> {
    let start = conf.find(BLOCK_HEAD)?;
    let end = start + conf[start..].find("\n}\n")? + 3;
    Some(start..end)
}

/// Se o bloco é exatamente o que o `keyboard_block_text` gera para os valores dele: ninguém o mexeu à mão (o
/// que um usuário muda costuma quebrar o alinhamento, um comentário ou a ordem). Para as contas de antes do
/// carimbo, que não têm como dizer o que foi gravado.
pub fn is_pristine_block(block: &str) -> bool {
    let Some(rest) = block.strip_prefix(BLOCK_HEAD) else { return false };
    let Some((file, body)) = rest.split_once(BLOCK_HEAD_END) else { return false };
    let mut text = String::new();
    for line in body.lines().skip(1) {
        let Some((key, value)) = line.trim().strip_prefix("xkb_").and_then(|l| l.split_once(' ')) else { continue };
        text.push_str(&format!("XKB{}=\"{}\"\n", key.to_ascii_uppercase(), value.trim()));
    }
    keyboard_block_text(Path::new(file), &text) == block
}

/// O teclado que a sessão do Sway acaba usando, no formato do /etc/default/keyboard (só XKBLAYOUT e
/// XKBVARIANT), dos arquivos do `config.d` na ordem em que o `include` os lê: o último `xkb_layout` e o último
/// `xkb_variant` de um `input type:keyboard` ou `input *` valem, como no Sway, que junta os blocos. Os blocos
/// de um teclado só (pelo identificador) ficam de fora. Sem layout, o Sway usa o us.
pub fn session_keyboard(confs: &[String]) -> String {
    let (mut layout, mut variant) = (String::new(), String::new());
    for conf in confs {
        // A chave do bloco pode vir na linha de baixo: `opening` guarda se a última linha foi um `input` nosso
        let (mut inside, mut opening) = (false, false);
        for line in conf.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let words: Vec<&str> = line.split_whitespace().map(|w| w.trim_matches('"')).collect();
            let was_opening = std::mem::replace(&mut opening, false);
            let setting: &[&str] = match words.as_slice() {
                ["{"] if was_opening => {
                    inside = true;
                    continue;
                }
                ["input", "type:keyboard" | "*", "{"] => {
                    inside = true;
                    continue;
                }
                ["input", "type:keyboard" | "*"] => {
                    opening = true;
                    continue;
                }
                ["input", "type:keyboard" | "*", rest @ ..] => rest,
                ["}"] => {
                    inside = false;
                    continue;
                }
                rest if inside => rest,
                _ => continue,
            };
            match setting {
                ["xkb_layout", value] => layout = value.to_string(),
                ["xkb_variant", value] => variant = value.to_string(),
                _ => {}
            }
        }
    }
    if layout.is_empty() {
        return String::new();
    }
    format!("XKBLAYOUT=\"{layout}\"\nXKBVARIANT=\"{variant}\"\n")
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
    fn us_international_in_any_layout() {
        assert!(is_us_intl("XKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n"));
        assert!(is_us_intl("XKBLAYOUT=\"us\"\nXKBVARIANT=\"altgr-intl\"\n"));
        assert!(is_us_intl("XKBLAYOUT=\"br,us\"\nXKBVARIANT=\"abnt2,intl\"\n"));
        // A variante é a da mesma posição
        assert!(!is_us_intl("XKBLAYOUT=\"us,br\"\nXKBVARIANT=\",intl\"\n"));
        for text in ["XKBLAYOUT=\"us\"\n", "XKBLAYOUT=\"br\"\nXKBVARIANT=\"abnt2\"\n",
                     "XKBLAYOUT=\"us\"\nXKBVARIANT=\"dvorak-intl\"\n", "XKBLAYOUT=\"pl\"\n", ""] {
            assert!(!is_us_intl(text), "{text}");
        }
        assert!(INTL_COMPOSE.contains("include \"%L\"\n<dead_acute> <c> : \"ç\" ccedilla\n<dead_acute> <C> : \"Ç\" Ccedilla\n"));
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
    fn our_block_is_found_and_checked() {
        let block = keyboard_block_text(Path::new("/etc/default/keyboard"), "XKBMODEL=\"pc105\"\nXKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n");
        let conf = format!("# --- Mouse ---\n# input type:pointer {{\n# }}\n{block}\n# depois\n");
        let range = find_block(&conf).unwrap();
        assert_eq!(&conf[range], block);
        assert!(is_pristine_block(&block));
        // O do Arch, pelo console
        assert!(is_pristine_block(&keyboard_block_text(Path::new("/etc/vconsole.conf"), "XKBMODEL=\"abnt2\"\nXKBLAYOUT=\"br\"\nXKBVARIANT=\"\"\n")));
        // Mexido: outro valor fora do alinhamento, uma linha a mais, um comentário
        assert!(!is_pristine_block(&block.replace("xkb_variant  intl", "xkb_variant intl")));
        assert!(!is_pristine_block(&block.replace("}\n", "    xkb_options caps:escape\n}\n")));
        assert!(!is_pristine_block(&block.replace("    xkb_model", "    # meu\n    xkb_model")));
        assert_eq!(find_block("# nada\n"), None);
        assert_eq!(find_block(&block.replace("}\n", "")), None);
    }

    #[test]
    fn the_session_keyboard_is_the_last_one_set() {
        let s = |confs: &[&str]| session_keyboard(&confs.iter().map(|c| c.to_string()).collect::<Vec<_>>());
        assert_eq!(s(&[]), "");
        assert_eq!(s(&["# input type:keyboard {\n#     xkb_layout  br,us\n# }\n"]), "");
        let ours = keyboard_block_text(Path::new("/etc/default/keyboard"), "XKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n");
        assert_eq!(s(&[&ours]), "XKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n");
        // Um bloco do usuário depois do nosso vale mais; só o layout muda, a variante fica
        assert_eq!(s(&[&format!("{ours}\ninput type:keyboard {{\n  xkb_layout br\n}}\n")]), "XKBLAYOUT=\"br\"\nXKBVARIANT=\"intl\"\n");
        // A forma de uma linha, o input *, e um arquivo depois do outro
        assert_eq!(s(&[&ours, "input * xkb_variant abnt2 # comentário\n"]), "XKBLAYOUT=\"us\"\nXKBVARIANT=\"abnt2\"\n");
        assert_eq!(s(&["input * {\n xkb_layout pl\n}\n"]), "XKBLAYOUT=\"pl\"\nXKBVARIANT=\"\"\n");
        // Valores entre aspas e a chave na linha de baixo, que o Sway aceita
        assert_eq!(s(&["input type:keyboard\n{\n xkb_layout \"us\"\n xkb_variant \"intl\"\n}\n"]), "XKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n");
        assert_eq!(s(&["input \"type:keyboard\" xkb_layout br\n"]), "XKBLAYOUT=\"br\"\nXKBVARIANT=\"\"\n");
        // Um teclado só, pelo identificador, não conta
        assert_eq!(s(&[&ours, "input \"1:1:AT_Translated_Set_2_keyboard\" {\n xkb_layout br\n}\n"]), "XKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n");
        assert!(is_us_intl(&s(&[&ours])));
    }

    #[test]
    fn the_keyboard_changed_last_wins() {
        let dir = tempdir("system");
        let (default, xorg, vconsole, map) = (dir.join("keyboard"), dir.join("00-keyboard.conf"), dir.join("vconsole.conf"), dir.join("kbd-model-map"));
        let get = || system_keyboard_from(&default, &xorg, &vconsole, &map);
        let at = |path: &Path, secs: u64| {
            let file = std::fs::File::options().write(true).open(path).unwrap();
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)).unwrap();
        };
        assert_eq!(get(), None);
        // O Debian: só o /etc/default/keyboard
        std::fs::write(&default, "XKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n").unwrap();
        assert_eq!(get(), Some((default.clone(), "XKBLAYOUT=\"us\"\nXKBVARIANT=\"intl\"\n".to_string())));
        // O Arch da nossa ISO: o /etc/default/keyboard da instalação, e o localectl grava o 00-keyboard.conf depois
        let xorg_br = "Section \"InputClass\"\n        Option \"XkbLayout\" \"br\"\n        Option \"XkbVariant\" \"abnt2\"\nEndSection\n";
        std::fs::write(&xorg, xorg_br).unwrap();
        at(&default, 1_000);
        at(&xorg, 2_000);
        assert_eq!(get(), Some((xorg.clone(), "XKBLAYOUT=\"br\"\nXKBVARIANT=\"abnt2\"\n".to_string())));
        // O Debian com um 00-keyboard.conf antigo: o dpkg-reconfigure (ou o localectl) grava o default depois
        at(&default, 3_000);
        assert_eq!(get().unwrap().0, default);
        // Do mesmo instante: o default
        at(&xorg, 3_000);
        assert_eq!(get().unwrap().0, default);
        // Um vconsole.conf só com o KEYMAP, mesmo mais novo, não vence um teclado X11: a tradução perde layouts
        // e opções (o "us,br" do localectl viraria só "us")
        std::fs::write(&map, "br-abnt2\tbr\tabnt2\t-\tterminate:ctrl_alt_bksp\n").unwrap();
        std::fs::write(&vconsole, "KEYMAP=br-abnt2\n").unwrap();
        at(&vconsole, 4_000);
        assert_eq!(get().unwrap().0, default);
        std::fs::remove_file(&default).unwrap();
        assert_eq!(get().unwrap().0, xorg);
        // Só ele: vale, como antes
        std::fs::remove_file(&xorg).unwrap();
        assert_eq!(get().unwrap().0, vconsole);
        // O Ubuntu 26.04: o localectl só reescreve o vconsole.conf, com as chaves XKB*, que valem mais que o KEYMAP
        std::fs::write(&default, "XKBMODEL=\"pc105\"\nXKBLAYOUT=\"us\"\nXKBVARIANT=\"\"\n").unwrap();
        std::fs::write(&xorg, "Section \"InputClass\"\n        Option \"XkbLayout\" \"us\"\nEndSection\n").unwrap();
        std::fs::write(&vconsole, "KEYMAP=us-acentos\nXKBMODEL=pc105\nXKBLAYOUT=us\nXKBVARIANT=intl\nXKBOPTIONS=\n").unwrap();
        at(&default, 5_000);
        at(&xorg, 5_000);
        at(&vconsole, 6_000);
        let (source, text) = get().unwrap();
        assert_eq!(source, vconsole);
        assert_eq!(text, "XKBMODEL=pc105\nXKBLAYOUT=us\nXKBVARIANT=intl\nXKBOPTIONS=\n");
        assert!(is_us_intl(&text));
        // Sem ele mais novo, o empate vai para o default, depois o 00-keyboard.conf
        at(&vconsole, 5_000);
        assert_eq!(get().unwrap().0, default);
        std::fs::remove_file(&default).unwrap();
        assert_eq!(get().unwrap().0, xorg);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
