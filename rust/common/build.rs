//! A versão que o --version mostra (swaydesk_common::cli::VERSION): a dos pacotes, lida do rust/Cargo.toml como o
//! packaging/version.sh a lê. A chave release de [workspace.metadata.swaydesk] (uma correção X.Y.Z.W numa branch
//! release/X.Y.Z, que não cabe no semver do Cargo), ou a version de [workspace.package]. Nunca a versão do Cargo
//! deste crate (tests/test_packaging.py confere): numa correção, a 0.1.3.1 apareceria como 0.1.3.

/// O valor da primeira linha `chave = "…"` (no começo da linha, como o awk do version.sh) da seção dada
fn key(cargo: &str, section: &str, name: &str) -> Option<String> {
    let mut inside = false;
    for line in cargo.lines() {
        if line.starts_with('[') {
            inside = line.trim_end() == section;
        } else if inside {
            let Some(rest) = line.strip_prefix(name) else { continue };
            let Some(value) = rest.trim_start().strip_prefix('=') else { continue };
            return value.split('"').nth(1).map(str::to_string);
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=../Cargo.toml");
    let cargo = std::fs::read_to_string("../Cargo.toml").expect("swaydesk-common: no ../Cargo.toml (the workspace's)");
    let version = key(&cargo, "[workspace.metadata.swaydesk]", "release")
        .filter(|release| !release.is_empty())
        .or_else(|| key(&cargo, "[workspace.package]", "version"))
        .filter(|version| !version.is_empty())
        .expect("swaydesk-common: no [workspace.package] version in ../Cargo.toml");
    println!("cargo:rustc-env=SWAYDESK_VERSION={version}");
}
