//! Guarda no cliphist o que não é texto nem imagem: um PDF ou um arquivo copiado como bytes. O porte do
//! `sway-clip-store`, que passa a vez a este binário (sway_rust).
//!
//! Roda num `wl-paste --watch` sem tipo (sway/config). Texto e imagem já têm os watchers deles
//! (`--type text` e `--type image`), então este só guarda quando a oferta não tem nenhum dos dois: senão o
//! mesmo item entraria duas vezes. Acima de MAX_BYTES não guarda: o cliphist descartaria de qualquer
//! jeito.
//!
//! O conteúdo fica na memória até o limite (5 MB) e vai inteiro para o `cliphist store`, com um prazo: se
//! o banco estiver preso, o watcher não fica parado. O que não é guardado é lido até o fim e descartado:
//! fechar a entrada antes faria o app que copiou receber um erro de escrita.
//!
//! Limite conhecido: o `wl-paste --watch` não passa a lista de tipos da cópia, então ela é pedida de novo
//! aqui (`--list-types`). Se outra cópia chegar nesse meio-tempo, a decisão vale pela nova.
use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

/// O próprio cliphist descarta o que passa disso (store, em cliphist.go)
const MAX_BYTES: usize = 5_000_000;
const CHUNK: usize = 65536;
const LIST_TIMEOUT: Duration = Duration::from_secs(5);
const STORE_TIMEOUT: Duration = Duration::from_secs(30);
const TEXT_TARGETS: [&str; 3] = ["UTF8_STRING", "STRING", "TEXT"];
const TEXT_SUFFIXES: [&str; 6] = ["script", "xml", "yaml", "csv", "ini", "pgp-keys"];

/// A mesma regra do wl-paste --type text (mime_type_is_text, no wl-clipboard 2.2.1).
fn is_text(kind: &str) -> bool {
    kind.starts_with("text/")
        || TEXT_TARGETS.contains(&kind)
        || kind.contains("json")
        || TEXT_SUFFIXES.iter().any(|s| kind.ends_with(s))
        || kind.contains("application/vnd.ms-publisher")
}

/// Os outros watchers já guardam: há um tipo de texto, ou de imagem (o --type image pega os tipos que
/// começam com "image").
fn covered(types: &[String]) -> bool {
    types.iter().any(|kind| is_text(kind) || kind.starts_with("image"))
}

/// Espera o filho até o prazo; passado o prazo, encerra e espera. None: estourou ou não deu para esperar.
fn wait_within(child: &mut std::process::Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn offered_types() -> Option<Vec<String>> {
    let mut child = Command::new("wl-paste").arg("--list-types").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let status = wait_within(&mut child, LIST_TIMEOUT)?;
    let text = reader.join().ok()?;
    status.success().then(|| text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
}

fn drain(input: &mut impl Read) {
    let mut buffer = vec![0u8; CHUNK];
    while matches!(input.read(&mut buffer), Ok(n) if n > 0) {}
}

/// O conteúdo, ou None se passar de MAX_BYTES (o resto é lido e descartado).
fn read_limited(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        match input.read(&mut buffer) {
            Ok(0) => return Some(data),
            Ok(n) => {
                if data.len() + n > MAX_BYTES {
                    drain(input);
                    return None;
                }
                data.extend_from_slice(&buffer[..n]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            // Um erro no meio: um pedaço não vira item no histórico
            Err(_) => return None,
        }
    }
}

/// O prazo do cliphist; SWAY_CLIP_STORE_TIMEOUT (segundos) só para os testes.
fn store_timeout() -> Duration {
    std::env::var("SWAY_CLIP_STORE_TIMEOUT").ok().and_then(|v| v.parse().ok()).map(Duration::from_secs_f64).unwrap_or(STORE_TIMEOUT)
}

fn store(data: Vec<u8>) -> u8 {
    let Ok(mut child) = Command::new("cliphist").arg("store").stdin(Stdio::piped()).stdout(Stdio::null()).spawn() else {
        return 1;
    };
    // A escrita numa linha à parte: um cliphist preso não lê, e o pipe cheio travaria a espera
    let mut stdin = child.stdin.take();
    let writer = std::thread::spawn(move || {
        if let Some(stdin) = stdin.as_mut() {
            let _ = stdin.write_all(&data);
        }
    });
    let status = wait_within(&mut child, store_timeout());
    // Morto no prazo, o pipe fecha e a escrita termina com erro
    let _ = writer.join();
    match status {
        Some(status) if status.success() => 0,
        _ => 1,
    }
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-clip-store\n",
    "    Stores in cliphist what wl-paste --watch hands it when it isn't text or an image (a PDF, a file copied as bytes).",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS;
    }
    let mut input = std::io::stdin().lock();
    // "sensitive" (senha) e "clear" ficam com o watcher de texto, que repassa ao cliphist
    if std::env::var("CLIPBOARD_STATE").unwrap_or_else(|_| "data".into()) != "data" {
        drain(&mut input);
        return ExitCode::SUCCESS;
    }
    match offered_types() {
        Some(types) if !covered(&types) => {}
        _ => {
            drain(&mut input);
            return ExitCode::SUCCESS;
        }
    }
    match read_limited(&mut input) {
        Some(data) if !data.is_empty() => ExitCode::from(store(data)),
        _ => ExitCode::SUCCESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn text_follows_wl_paste() {
        for kind in ["text/plain;charset=utf-8", "UTF8_STRING", "STRING", "TEXT", "application/json", "application/javascript", "application/xml", "application/pgp-keys", "text/uri-list"] {
            assert!(is_text(kind), "{kind}");
        }
        for kind in ["application/pdf", "application/octet-stream", "image/png", "x-special/gnome-copied-files"] {
            assert!(!is_text(kind), "{kind}");
        }
    }

    #[test]
    fn covered_by_text_or_image() {
        assert!(covered(&types(&["text/plain", "application/x-foo"])));
        assert!(covered(&types(&["image/png"])));
        assert!(!covered(&types(&["application/pdf"])));
        assert!(!covered(&types(&[])));
    }

    #[test]
    fn read_limited_stops_above_the_limit() {
        let exact = vec![7u8; MAX_BYTES];
        assert_eq!(read_limited(&mut exact.as_slice()).map(|d| d.len()), Some(MAX_BYTES));
        let big = vec![7u8; MAX_BYTES + 1];
        assert_eq!(read_limited(&mut big.as_slice()), None);
        assert_eq!(read_limited(&mut [].as_slice()), Some(Vec::new()));
    }
}
