//! O `setup` do binário, com o home-copy/ do repositório no papel do skel do pacote: o primeiro login copia o
//! que falta e gera os arquivos da cor, inclusive a folha da Waybar da sessão do pacote; o segundo não
//! sobrescreve nada do usuário.
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("swaydesk-setup-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn setup(home: &Path, keyboard: &Path) -> std::process::Output {
    let repo = repo();
    Command::new(env!("CARGO_BIN_EXE_swaydesk-desktop-config"))
        .arg("setup")
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("SWAYDESK_SKEL", repo.join("home-copy"))
        .env("SWAY_DESKTOP_SHARE", repo.join("home/.local/share/sway-desktop"))
        .env("SWAY_DESKTOP_KEYBOARD", keyboard)
        .output()
        .unwrap()
}

/// O gtk_theme da config.toml do skel.
fn skel_theme() -> String {
    let text = std::fs::read_to_string(repo().join("home-copy/.config/sway-desktop/config.toml")).unwrap();
    let line = text.lines().find_map(|l| l.strip_prefix("gtk_theme = \"")).expect("gtk_theme no skel");
    line.trim_end_matches('"').to_string()
}

#[test]
fn first_login_copies_the_skel_and_generates() {
    let dir = tempdir();
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let keyboard = dir.join("keyboard");
    std::fs::write(&keyboard, "XKBLAYOUT=\"br\"\nXKBVARIANT=\"abnt2\"\n").unwrap();
    // O tema GTK do skel instalado no ~/.themes: sem ele, o setup troca o tema pelo de reserva, e o resultado
    // dependeria dos temas da máquina que compila (o Mint-Y só existe no Mint)
    let theme = skel_theme();
    let css = home.join(".themes").join(&theme).join("gtk-3.0/gtk.css");
    std::fs::create_dir_all(css.parent().unwrap()).unwrap();
    std::fs::write(&css, "@define-color theme_selected_bg_color #1f9ede;\n").unwrap();

    let out = setup(&home, &keyboard);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    for rel in [".config/sway-desktop/config.toml", ".config/gammastep/config.ini", ".local/share/wallpaper/current.jpg"] {
        assert_eq!(std::fs::read(home.join(rel)).unwrap(), std::fs::read(repo().join("home-copy").join(rel)).unwrap(), "{rel}");
    }
    let local = std::fs::read_to_string(home.join(".config/sway/config.d/50-local.conf")).unwrap();
    assert!(local.ends_with("input type:keyboard {\n    xkb_layout   br\n    xkb_variant  abnt2\n}\n"), "{local}");
    let pinned = std::fs::read_to_string(home.join(".local/share/sway-dock/pinned")).unwrap();
    assert!(pinned.lines().any(|l| l == "kitty"), "{pinned}");
    // O generate: as cores da Waybar e a folha da sessão do pacote, que as importa
    assert!(home.join(".config/waybar/accent.css").is_file());
    assert!(home.join(".config/waybar/fonts.css").is_file());
    let wrapper = std::fs::read_to_string(home.join(".config/swaydesk-desktop/waybar/style.css")).unwrap();
    assert!(wrapper.contains("@import url(\"../../waybar/accent.css\");\n@import url(\"../../waybar/fonts.css\");\n@import url(\"/usr/share/swaydesk-desktop/waybar/style.css\");\n"), "{wrapper}");

    // O segundo login: o que o usuário mudou fica
    std::fs::write(home.join(".config/sway-desktop/config.toml"), "accent = \"#7c3aed\"\n").unwrap();
    std::fs::remove_file(home.join(".config/gammastep/config.ini")).unwrap();
    let out = setup(&home, &keyboard);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_to_string(home.join(".config/sway-desktop/config.toml")).unwrap(), "accent = \"#7c3aed\"\n");
    assert!(home.join(".config/gammastep/config.ini").is_file(), "o que falta volta");
    assert!(std::fs::read_to_string(home.join(".config/waybar/accent.css")).unwrap().contains("7c3aed"));
    let _ = std::fs::remove_dir_all(&dir);
}
