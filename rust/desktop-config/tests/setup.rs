//! O `setup` do binário, com o home-copy/ do repositório no papel do skel do pacote: o primeiro login copia o
//! que falta e gera os arquivos da cor, inclusive a folha da Waybar da sessão do pacote; o segundo não
//! sobrescreve nada do usuário.
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Uma pasta por teste: os testes rodam em paralelo no mesmo processo
fn tempdir() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("swaydesk-setup-cli-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn setup(home: &Path, keyboard: &Path) -> std::process::Output {
    setup_with_path(home, keyboard, "/usr/bin:/bin")
}

fn setup_with_path(home: &Path, keyboard: &Path, path: &str) -> std::process::Output {
    setup_with_env(home, keyboard, path, &[])
}

fn setup_with_env(home: &Path, keyboard: &Path, path: &str, env: &[(&str, &Path)]) -> std::process::Output {
    let repo = repo();
    Command::new(env!("CARGO_BIN_EXE_swaydesk-desktop-config"))
        .arg("setup")
        .env_clear()
        .envs(env.iter().copied())
        .env("HOME", home)
        .env("PATH", path)
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
    // Conta nova: a config.toml do skel (.config/sway-desktop no repositório) vai para a pasta nova, sem o link
    for (rel, skel) in [
        (".config/swaydesk/config.toml", ".config/sway-desktop/config.toml"),
        (".config/gammastep/config.ini", ".config/gammastep/config.ini"),
        (".local/share/wallpaper/current.jpg", ".local/share/wallpaper/current.jpg"),
    ] {
        assert_eq!(std::fs::read(home.join(rel)).unwrap(), std::fs::read(repo().join("home-copy").join(skel)).unwrap(), "{rel}");
    }
    assert!(std::fs::symlink_metadata(home.join(".config/sway-desktop")).is_err());
    let local = std::fs::read_to_string(home.join(".config/sway/config.d/50-local.conf")).unwrap();
    assert!(local.ends_with("input type:keyboard {\n    xkb_layout   br\n    xkb_variant  abnt2\n}\n"), "{local}");
    let pinned = std::fs::read_to_string(home.join(".local/share/sway-dock/pinned")).unwrap();
    assert!(pinned.lines().any(|l| l == "kitty"), "{pinned}");
    // O generate: as cores da Waybar e a folha da sessão do pacote, que as importa
    assert!(home.join(".config/waybar/accent.css").is_file());
    assert!(home.join(".config/waybar/fonts.css").is_file());
    let wrapper = std::fs::read_to_string(home.join(".config/swaydesk/waybar/style.css")).unwrap();
    assert!(wrapper.contains("@import url(\"../../waybar/accent.css\");\n@import url(\"../../waybar/fonts.css\");\n@import url(\"/usr/share/swaydesk-desktop/waybar/style.css\");\n"), "{wrapper}");

    // O segundo login: o que o usuário mudou fica
    std::fs::write(home.join(".config/swaydesk/config.toml"), "accent = \"#7c3aed\"\n").unwrap();
    std::fs::remove_file(home.join(".config/gammastep/config.ini")).unwrap();
    let out = setup(&home, &keyboard);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_to_string(home.join(".config/swaydesk/config.toml")).unwrap(), "accent = \"#7c3aed\"\n");
    assert!(home.join(".config/gammastep/config.ini").is_file(), "o que falta volta");
    assert!(std::fs::read_to_string(home.join(".config/waybar/accent.css")).unwrap().contains("7c3aed"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_login_after_0_1_3_moves_the_folders_and_keeps_the_config() {
    let dir = tempdir();
    let home = dir.join("home");
    let keyboard = dir.join("sem-teclado");
    std::fs::create_dir_all(home.join(".config/sway-desktop")).unwrap();
    std::fs::write(home.join(".config/sway-desktop/config.toml"), "accent = \"#7c3aed\"\n").unwrap();
    std::fs::create_dir_all(home.join(".config/swaydesk-desktop/waybar")).unwrap();
    std::fs::write(home.join(".config/swaydesk-desktop/waybar/config.jsonc"), "{}").unwrap();
    std::fs::create_dir_all(home.join(".local/state/sway-desktop")).unwrap();
    std::fs::write(home.join(".local/state/sway-desktop/accent-auto"), "#abcdef").unwrap();
    let out = setup(&home, &keyboard);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_link(home.join(".config/sway-desktop")).unwrap(), PathBuf::from("swaydesk"));
    assert_eq!(std::fs::read_to_string(home.join(".config/swaydesk/config.toml")).unwrap(), "accent = \"#7c3aed\"\n");
    assert_eq!(std::fs::read_link(home.join(".config/swaydesk-desktop/waybar")).unwrap(), PathBuf::from("../swaydesk/waybar"));
    assert_eq!(std::fs::read_to_string(home.join(".local/state/swaydesk/accent-auto")).unwrap(), "#abcdef");
    // O generate do setup já usa a config migrada e grava a folha da Waybar na pasta nova
    assert!(std::fs::read_to_string(home.join(".config/waybar/accent.css")).unwrap().contains("7c3aed"));
    assert!(home.join(".config/swaydesk/waybar/style.css").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Uma pasta com um executável falso de cada nome, para o PATH do setup.
fn fake_bin(dir: &Path, names: &[&str]) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in names {
        std::fs::write(bin.join(name), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

#[test]
fn the_first_pin_is_the_installed_file_manager_not_nemo() {
    for (installed, expected) in [("thunar", "thunar"), ("nautilus", "org.gnome.Nautilus")] {
        // Uma pasta própria: o tempdir() é o mesmo do outro teste, que roda em paralelo e a apagaria
        let dir = std::env::temp_dir().join(format!("swaydesk-setup-pin-{}-{installed}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let bin = fake_bin(&dir, &[installed]);
        let out = setup_with_path(&home, &dir.join("sem-teclado"), &format!("{}:/usr/bin:/bin", bin.display()));
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let pinned = std::fs::read_to_string(home.join(".local/share/sway-dock/pinned")).unwrap();
        assert_eq!(pinned.lines().next(), Some(expected), "{installed}: {pinned}");
        assert!(!pinned.lines().any(|l| l == "nemo"), "{installed}: {pinned}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn an_old_pinned_file_with_nemo_not_installed_is_migrated_once() {
    let dir = std::env::temp_dir().join(format!("swaydesk-setup-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("home");
    let pinned = home.join(".local/share/sway-dock/pinned");
    std::fs::create_dir_all(pinned.parent().unwrap()).unwrap();
    // O que a ISO 0.1.x deixava: o modelo cru e o instalador
    std::fs::write(&pinned, "nemo\nkitty\nswaydesk-install\n").unwrap();
    let bin = fake_bin(&dir, &["thunar"]);
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let out = setup_with_path(&home, &dir.join("sem-teclado"), &path);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_to_string(&pinned).unwrap(), "thunar\nkitty\nswaydesk-install\n");
    // Com o Nemo instalado, o "nemo" é do usuário e fica
    std::fs::write(&pinned, "nemo\nkitty\n").unwrap();
    let bin = fake_bin(&dir, &["thunar", "nemo"]);
    let out = setup_with_path(&home, &dir.join("sem-teclado"), &format!("{}:/usr/bin:/bin", bin.display()));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_to_string(&pinned).unwrap(), "nemo\nkitty\n");
    let _ = std::fs::remove_dir_all(&dir);
}

// O 50-local.conf de uma conta nova vem na língua da sessão: espanhol com o LANG es_ES, inglês sem língua
#[test]
fn the_local_file_comes_in_the_language_of_the_session() {
    let base = std::env::temp_dir().join(format!("swaydesk-setup-lang-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let locale = repo().join("home/.local/share/locale");
    for (lang, start) in [("es_ES.UTF-8", "# Ajustes solo de esta máquina"), ("C", "# Settings for this machine only")] {
        let home = base.join(lang);
        std::fs::create_dir_all(&home).unwrap();
        let out = setup_with_env(&home, &base.join("sem-teclado"), "/usr/bin:/bin",
                                 &[("LANG", Path::new(lang)), ("SWAY_I18N_LOCALEDIR", &locale)]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let local = std::fs::read_to_string(home.join(".config/sway/config.d/50-local.conf")).unwrap();
        assert!(local.starts_with(start), "{lang}: {local}");
    }
    let _ = std::fs::remove_dir_all(&base);
}
