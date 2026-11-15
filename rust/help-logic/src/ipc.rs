//! O GET_CONFIG do IPC do Sway: o texto do config principal que está carregado.
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

const MAGIC: &[u8; 6] = b"i3-ipc";
const GET_CONFIG: u32 = 9;
const TIMEOUT: Duration = Duration::from_secs(2);
/// A maior resposta aceita: o config é texto pequeno, e o tamanho vem do outro lado.
const MAX_REPLY: usize = 4 << 20;

pub fn get_config(socket: &Path) -> io::Result<String> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.write_all(&request(GET_CONFIG))?;
    let mut header = [0u8; 14];
    stream.read_exact(&mut header)?;
    let (len, kind) = reply_header(&header)?;
    if kind != GET_CONFIG {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected reply type"));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    config_of(&body)
}

pub fn request(kind: u32) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&0u32.to_ne_bytes());
    out.extend_from_slice(&kind.to_ne_bytes());
    out
}

/// O tamanho e o tipo da resposta.
pub fn reply_header(header: &[u8; 14]) -> io::Result<(usize, u32)> {
    if &header[..6] != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not an i3-ipc reply"));
    }
    let len = u32::from_ne_bytes(header[6..10].try_into().unwrap()) as usize;
    let kind = u32::from_ne_bytes(header[10..14].try_into().unwrap());
    if len > MAX_REPLY {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "reply too large"));
    }
    Ok((len, kind))
}

pub fn config_of(body: &[u8]) -> io::Result<String> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    value
        .get("config")
        .and_then(|c| c.as_str())
        .map(str::to_string)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no config in the reply"))
}
