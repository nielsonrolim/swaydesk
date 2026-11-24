//! A base do dconf que o `set` grava, rodado de fora da sessão (ssh, cron: sem o DCONF_PROFILE no ambiente).
//! Só com os pacotes não há perfil no ~: vale o do Sway em execução, senão o tema ia para a base normal, que a
//! sessão não lê (visto na VM: o portal e os apps GTK 4 ficavam com o tema antigo).
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Uma pasta por teste: os testes rodam em paralelo no mesmo processo
fn tempdir() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("swaydesk-theme-dconf-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Um HOME com um tema GTK, um gsettings falso que anota o DCONF_PROFILE de cada gravação e um /proc falso,
/// com um processo sway nosso quando `sway_profile` vem (Some("") = sway sem o perfil).
fn profiles_written(sway_profile: Option<&str>) -> Vec<String> {
    profiles_written_with(sway_profile, None)
}

/// O mesmo, com o arquivo do sway-session no XDG_RUNTIME_DIR quando `session_file` vem (o conteúdo; "{perfil}"
/// vira um perfil que existe) e, nesse caso, o sway com o environ ilegível, como o do Arch (cap_sys_nice).
fn profiles_written_with(sway_profile: Option<&str>, session_file: Option<&str>) -> Vec<String> {
    let dir = tempdir();
    let home = dir.join("home");
    let theme = home.join(".themes/Novo-Dark/gtk-3.0");
    std::fs::create_dir_all(&theme).unwrap();
    std::fs::write(theme.join("gtk.css"), "@define-color accent_color #112233;\n").unwrap();
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.join("perfis");
    let gsettings = bin.join("gsettings");
    std::fs::write(&gsettings, format!("#!/bin/sh\necho \"[$DCONF_PROFILE]\" >> '{}'\n", log.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gsettings, std::fs::Permissions::from_mode(0o755)).unwrap();
    let proc = dir.join("proc");
    std::fs::create_dir_all(&proc).unwrap();
    if let Some(profile) = sway_profile {
        let pid = proc.join("4242");
        std::fs::create_dir_all(&pid).unwrap();
        std::fs::write(pid.join("comm"), "sway\n").unwrap();
        let mut environ = b"XDG_CURRENT_DESKTOP=sway\0".to_vec();
        if !profile.is_empty() {
            environ.extend_from_slice(format!("DCONF_PROFILE={profile}\0").as_bytes());
        }
        if session_file.is_none() {
            std::fs::write(pid.join("environ"), environ).unwrap();
        }
        // O que se lê do sway do Arch: o comm e o uid real
        // SAFETY: getuid não falha nem toca memória
        let uid = unsafe { libc::getuid() };
        std::fs::write(pid.join("status"), format!("Name:\tsway\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n")).unwrap();
    }
    let runtime = dir.join("run");
    std::fs::create_dir_all(&runtime).unwrap();
    if let Some(content) = session_file {
        let profile = dir.join("perfil-sway");
        std::fs::write(&profile, "user-db:sway\nuser-db:user\n").unwrap();
        std::fs::create_dir_all(runtime.join("swaydesk")).unwrap();
        let content = content.replace("{perfil}", &profile.display().to_string());
        std::fs::write(runtime.join("swaydesk/dconf-profile"), content).unwrap();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_swaydesk-theme"))
        .args(["set", "Novo-Dark"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("SWAY_THEME_GSETTINGS", &gsettings)
        .env("SWAY_DESKTOP_PROC", &proc)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("SWAY_DESKTOP_SHARE", repo().join("home/.local/share/sway-desktop"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let written = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    written.lines().map(str::to_string).collect()
}

#[test]
fn outside_a_package_session_the_theme_goes_to_the_sway_database() {
    let sway = repo().join("home/.local/share/sway-desktop/dconf/sway");
    let written = profiles_written(Some("/usr/share/sway-desktop/dconf/sway"));
    assert!(!written.is_empty());
    assert!(written.iter().all(|p| *p == format!("[{}]", sway.display())), "{written:?}");
}

#[test]
fn without_a_sway_with_the_profile_the_normal_database_stays() {
    // Nenhum Sway (cron, tty), ou um Sway aberto sem o sway-session: a base normal, como antes
    for sway_profile in [None, Some("")] {
        let written = profiles_written(sway_profile);
        assert!(!written.is_empty());
        assert!(written.iter().all(|p| p == "[]"), "{sway_profile:?}: {written:?}");
    }
}

#[test]
fn on_arch_the_session_file_names_the_database() {
    // O environ do sway do Arch não se lê: vale o arquivo que o sway-session gravou no login
    let sway = repo().join("home/.local/share/sway-desktop/dconf/sway");
    let written = profiles_written_with(Some("/usr/share/sway-desktop/dconf/sway"), Some("4242\n{perfil}\n"));
    assert!(!written.is_empty());
    assert!(written.iter().all(|p| *p == format!("[{}]", sway.display())), "{written:?}");
    // Vazio: a sessão não tem o perfil, a base normal
    let written = profiles_written_with(Some(""), Some("4242\n\n"));
    assert!(!written.is_empty());
    assert!(written.iter().all(|p| p == "[]"), "{written:?}");
    // O Sway que gravou já saiu (outro desktop depois do logout, com a pasta ainda de pé): a base normal
    let written = profiles_written_with(None, Some("4242\n{perfil}\n"));
    assert!(!written.is_empty());
    assert!(written.iter().all(|p| p == "[]"), "{written:?}");
}
