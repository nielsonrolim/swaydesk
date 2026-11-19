//! O mínimo do IPC do Sway que o módulo usa: GET_INPUTS e a assinatura dos eventos de input.
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

const MAGIC: &[u8; 6] = b"i3-ipc";
pub const SUBSCRIBE: u32 = 2;
pub const GET_INPUTS: u32 = 100;
/// O evento de input: o bit alto marca evento, e o tipo é 21.
pub const EVENT_INPUT: u32 = 0x8000_0015;
/// A maior mensagem aceita: o tamanho vem do outro lado.
const MAX_MESSAGE: usize = 8 << 20;

pub fn message(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    out.extend_from_slice(&kind.to_ne_bytes());
    out.extend_from_slice(payload);
    out
}

/// O tipo e o corpo da próxima mensagem do Sway.
pub fn read(stream: &mut UnixStream) -> io::Result<(u32, Vec<u8>)> {
    let mut header = [0u8; 14];
    stream.read_exact(&mut header)?;
    if &header[..6] != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not an i3-ipc message"));
    }
    let len = u32::from_ne_bytes(header[6..10].try_into().unwrap()) as usize;
    let kind = u32::from_ne_bytes(header[10..14].try_into().unwrap());
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    Ok((kind, body))
}

/// Os inputs agora, numa conexão própria (a outra só recebe eventos).
pub fn get_inputs(socket: &Path) -> io::Result<serde_json::Value> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&message(GET_INPUTS, b""))?;
    let (kind, body) = read(&mut stream)?;
    if kind != GET_INPUTS {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected reply type"));
    }
    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Uma conexão assinada nos eventos de input; cada `read` dela devolve o próximo evento.
pub fn subscribe_input(socket: &Path) -> io::Result<UnixStream> {
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(&message(SUBSCRIBE, br#"["input"]"#))?;
    let (kind, body) = read(&mut stream)?;
    let ok = kind == SUBSCRIBE
        && serde_json::from_slice::<serde_json::Value>(&body).ok().and_then(|v| v.get("success")?.as_bool()) == Some(true);
    if !ok {
        return Err(io::Error::new(io::ErrorKind::Other, "subscribe failed"));
    }
    Ok(stream)
}
