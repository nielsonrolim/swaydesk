//! As capas: arquivo local, embutida (data:) ou da rede, com o cache em `~/.cache/waybar-media` (o mesmo
//! do Python). O porte da parte de capas do `waybar_media.py`.
//!
//! SEGURANÇA: o artUrl de um navegador vem da página, então:
//! - só se baixa de um endereço da internet (todos os endereços do nome são globais), também depois de
//!   cada redirecionamento ([`public_url`]);
//! - a resposta só vale se o Content-Type for image/* e os primeiros bytes forem de PNG, JPEG, GIF ou WebP
//!   ([`image_type`]): o carregador de imagens nunca recebe outro formato;
//! - até 5 MB, de arquivo comum (sem travar num FIFO).
//!
//! A conexão usa exatamente os endereços conferidos: o resolvedor do agente devolve só endereços globais e
//! recusa o nome se algum não for ([`public_resolver`]). Sem uma segunda resolução, um DNS que muda de
//! resposta (rebinding) não leva a conexão para a rede local, o que o Python aceitava como risco.
//!
//! Os proxies do ambiente (http_proxy) não são usados, ao contrário do urllib: um proxy resolveria o nome
//! do lado dele e furaria a conferência. Numa rede que exige proxy, a capa da rede não chega.
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gio::glib;

pub const ART_MAX_BYTES: usize = 5 * 1024 * 1024;
/// Capas guardadas no cache; as mais antigas saem
pub const ART_KEEP: usize = 50;
const MAX_REDIRECTS: u32 = 5;

pub fn art_cache() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| swaydesk_common::config::home().join(".cache")).join("waybar-media")
}

/// O arquivo da capa: o próprio, num file://; o do cache, num http(s) (pode ainda não existir). None:
/// sem capa ou num esquema que não se baixa.
pub fn art_path(url: &str, cache: &Path) -> Option<PathBuf> {
    if url.is_empty() {
        return None;
    }
    let parsed = url::Url::parse(url).ok()?;
    match parsed.scheme() {
        "file" => {
            // O caminho decodificado (%20 → espaço), como o unquote do Python
            let path = percent_decode(parsed.path());
            Some(PathBuf::from(<std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(path)))
        }
        "http" | "https" => Some(cache.join(glib::compute_checksum_for_data(glib::ChecksumType::Sha256, url.as_bytes())?.as_str())),
        _ => None,
    }
}

fn percent_decode(text: &str) -> Vec<u8> {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Se a capa é de um tipo que se carrega: arquivo, rede ou embutida (data:, como no mpv-mpris 1.x).
pub fn has_art(url: &str) -> bool {
    !url.is_empty() && (art_path(url, Path::new("/")).is_some() || url.starts_with("data:"))
}

/// Os bytes de uma capa embutida (data:...;base64,...), até o limite. O tipo declarado não vale: o
/// mpv-mpris diz jpeg num PNG, e quem decide é o [`image_type`].
pub fn data_url_bytes(url: &str) -> Option<Vec<u8>> {
    let (header, payload) = url.split_once(',')?;
    if !header.to_lowercase().ends_with(";base64") || payload.len() > ART_MAX_BYTES * 4 / 3 + 4 {
        return None;
    }
    // O b64decode do Python com validate=False ignora o que não é do alfabeto; aqui, o mesmo
    let clean: String = payload.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')).collect();
    if clean.is_empty() {
        return None;
    }
    let data = glib::base64_decode(&clean);
    (!data.is_empty()).then_some(data)
}

/// O formato pelos primeiros bytes; None fora dos que se aceitam (PNG, JPEG, GIF, WebP).
pub fn image_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if data.starts_with(b"\xff\xd8\xff") {
        return Some("jpeg");
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

/// Um endereço da internet: o `is_global` do `ipaddress` do Python, um pouco mais estrito (multicast e o
/// que não é unicast global ficam de fora).
pub fn is_global(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_global_v4(v4),
        IpAddr::V6(v6) => is_global_v6(v6),
    }
}

fn in_v4(ip: Ipv4Addr, net: [u8; 4], prefix: u32) -> bool {
    let (a, n) = (u32::from(ip), u32::from(Ipv4Addr::from(net)));
    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    a & mask == n & mask
}

fn is_global_v4(ip: Ipv4Addr) -> bool {
    // As redes privadas e reservadas do ipaddress (is_private) e o 100.64/10, mais o multicast
    const BLOCKED: [([u8; 4], u32); 16] = [
        ([0, 0, 0, 0], 8),
        ([10, 0, 0, 0], 8),
        ([100, 64, 0, 0], 10),
        ([127, 0, 0, 0], 8),
        ([169, 254, 0, 0], 16),
        ([172, 16, 0, 0], 12),
        ([192, 0, 0, 0], 24),
        ([192, 0, 2, 0], 24),
        ([192, 88, 99, 0], 24),
        ([192, 168, 0, 0], 16),
        ([198, 18, 0, 0], 15),
        ([198, 51, 100, 0], 24),
        ([203, 0, 113, 0], 24),
        ([224, 0, 0, 0], 4),
        ([240, 0, 0, 0], 4),
        ([255, 255, 255, 255], 32),
    ];
    !BLOCKED.iter().any(|(net, prefix)| in_v4(ip, *net, *prefix))
}

fn in_v6(ip: Ipv6Addr, net: Ipv6Addr, prefix: u32) -> bool {
    let (a, n) = (u128::from(ip), u128::from(net));
    let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
    a & mask == n & mask
}

fn is_global_v6(ip: Ipv6Addr) -> bool {
    // Um IPv4 mapeado vale pelo IPv4; o NAT64 (64:ff9b::/96, de uma rede só IPv6 com DNS64), também
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_global_v4(v4);
    }
    if in_v6(ip, "64:ff9b::".parse().unwrap(), 96) {
        return is_global_v4(Ipv4Addr::from((u128::from(ip) & 0xffff_ffff) as u32));
    }
    // Só o unicast global (2000::/3), e dentro dele sem as faixas especiais
    if !in_v6(ip, "2000::".parse().unwrap(), 3) {
        return false;
    }
    let blocked: [(&str, u32); 4] = [("2001::", 23), ("2001:db8::", 32), ("2002::", 16), ("3fff::", 20)];
    // 2001::/23 inclui o Teredo e o ORCHID; as exceções globais dele (o ipaddress as aceita) ficam fora: o
    // mais estrito não abre nada
    !blocked.iter().any(|(net, prefix)| in_v6(ip, net.parse().unwrap(), *prefix))
}

/// Só http(s) para um endereço da internet: todos os endereços do nome precisam ser globais.
pub fn public_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else { return false };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host() else { return false };
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<IpAddr> = match host {
        url::Host::Ipv4(ip) => vec![IpAddr::V4(ip)],
        url::Host::Ipv6(ip) => vec![IpAddr::V6(ip)],
        url::Host::Domain(name) => match (name, port).to_socket_addrs() {
            Ok(found) => found.map(|a| a.ip()).collect(),
            Err(_) => return false,
        },
    };
    !addrs.is_empty() && addrs.into_iter().all(is_global)
}

/// Os bytes de um arquivo comum até o limite; None se for outra coisa (FIFO, aparelho), grande demais ou
/// não abrir. Sem bloquear num FIFO.
pub fn read_limited(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > ART_MAX_BYTES as u64 {
        return None;
    }
    // O_NONBLOCK: abrir um FIFO sem escritor não trava
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(path).ok()?;
    let ft = file.metadata().ok()?.file_type();
    if !ft.is_file() || ft.is_fifo() {
        return None;
    }
    let mut data = Vec::new();
    file.take(ART_MAX_BYTES as u64 + 1).read_to_end(&mut data).ok()?;
    (data.len() <= ART_MAX_BYTES).then_some(data)
}

/// Uma resposta da rede: o tipo e os bytes, até o limite.
pub struct Fetched {
    pub content_type: String,
    pub body: Vec<u8>,
}

/// O resolvedor da conexão: resolve uma vez e só devolve se todos os endereços forem globais.
pub fn public_resolver(netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
    let addrs: Vec<std::net::SocketAddr> = netloc.to_socket_addrs()?.collect();
    if addrs.is_empty() || !addrs.iter().all(|a| is_global(a.ip())) {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "not a public address"));
    }
    Ok(addrs)
}

/// Baixa só de endereços públicos, conferindo cada redirecionamento (até 5).
pub fn open_public(url: &str, check: &dyn Fn(&str) -> bool) -> Option<Fetched> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(5)).redirects(0).resolver(public_resolver).build();
    let mut current = url::Url::parse(url).ok()?;
    for _ in 0..=MAX_REDIRECTS {
        if !check(current.as_str()) {
            return None;
        }
        let response = match agent.get(current.as_str()).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(_) => return None,
        };
        if (300..400).contains(&response.status()) {
            current = current.join(response.header("location")?).ok()?;
            continue;
        }
        if response.status() != 200 {
            return None;
        }
        let content_type = response.content_type().to_lowercase();
        let mut body = Vec::new();
        response.into_reader().take(ART_MAX_BYTES as u64 + 1).read_to_end(&mut body).ok()?;
        return Some(Fetched { content_type, body });
    }
    None
}

/// Os bytes da capa, de um formato aceito, ou None. Fora da linha principal: lê arquivos e a rede. O que
/// vem da rede fica no cache, e a capa reusada é marcada como recente. `fetch` e `check` trocam a rede e
/// a conferência do endereço nos testes.
pub fn load_art(url: &str, cache: &Path, fetch: &dyn Fn(&str) -> Option<Fetched>, check: &dyn Fn(&str) -> bool) -> Option<Vec<u8>> {
    if url.starts_with("data:") {
        let data = data_url_bytes(url)?;
        return (data.len() <= ART_MAX_BYTES && image_type(&data).is_some()).then_some(data);
    }
    let path = art_path(url, cache)?;
    if url::Url::parse(url).ok()?.scheme() == "file" {
        return read_limited(&path).filter(|d| image_type(d).is_some());
    }
    if let Some(data) = read_limited(&path).filter(|d| image_type(d).is_some()) {
        touch(&path);
        return Some(data);
    }
    if !check(url) {
        return None;
    }
    let fetched = fetch(url)?;
    if !fetched.content_type.starts_with("image/") {
        return None;
    }
    let data = fetched.body;
    if data.is_empty() || data.len() > ART_MAX_BYTES || image_type(&data).is_none() {
        return None;
    }
    save_art(&path, &data);
    Some(data)
}

/// A capa da rede com a conferência de verdade.
pub fn load_art_public(url: &str) -> Option<Vec<u8>> {
    load_art(url, &art_cache(), &|u| open_public(u, &public_url), &public_url)
}

fn touch(path: &Path) {
    let _ = std::fs::File::open(path).and_then(|f| f.set_modified(std::time::SystemTime::now()));
}

pub fn save_art(path: &Path, data: &[u8]) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = dir.join(format!(".art.{}.{:x}", std::process::id(), glib::random_int()));
    if std::fs::write(&tmp, data).is_err() || std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    prune_art(dir, ART_KEEP);
}

/// Deixa as `keep` mais recentes. É só faxina: outra linha apagando ao mesmo tempo, ou o cache sumindo,
/// não pode impedir a capa de aparecer.
pub fn prune_art(cache: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(cache) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, old) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(old);
    }
}
