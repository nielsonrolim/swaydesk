//! O protocolo do greetd: cada mensagem é um u32 no endian da máquina com o tamanho, seguido do JSON
//! (https://man.sr.ht/~kennylevinsen/greetd/, greetd-ipc(7)). O greetd faz o PAM; a tela só repassa as
//! perguntas dele e as respostas de quem digita.
//!
//! O [`Client`] esconde o vai e vem: as mensagens `info` e `error` do PAM no meio da conversa são
//! juntadas em [`Step::notes`] e respondidas sozinhas, e um erro cancela a sessão no greetd, para a
//! próxima tentativa começar do zero.
use std::io::{Read, Write};

use serde::Serialize;

/// O maior JSON que aceitamos do greetd (as respostas dele têm poucas centenas de bytes).
const MAX_MESSAGE: u32 = 64 * 1024;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request<'a> {
    CreateSession { username: &'a str },
    PostAuthMessageResponse { response: Option<&'a str> },
    StartSession { cmd: &'a [String], env: &'a [String] },
    CancelSession,
}

/// Uma resposta do greetd.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    Success,
    /// `auth` é o `auth_error` (senha errada), em oposição a um erro qualquer
    Error { auth: bool, description: String },
    /// `kind` é visible, secret, info ou error
    AuthMessage { kind: String, text: String },
}

/// Grava uma mensagem: o tamanho e o JSON. O buffer é zerado depois, porque pode levar a senha; ele já
/// nasce do tamanho do pior caso (cada caractere escapado como \uXXXX), para não crescer e deixar para
/// trás uma cópia menor sem zerar.
fn write_message(conn: &mut impl Write, request: &Request) -> std::io::Result<()> {
    let longest = match request {
        Request::CreateSession { username } => username.len(),
        Request::PostAuthMessageResponse { response } => response.map_or(0, str::len),
        Request::StartSession { cmd, env } => cmd.iter().chain(env.iter()).map(String::len).sum::<usize>() + 4 * (cmd.len() + env.len()),
        Request::CancelSession => 0,
    };
    let mut body = Vec::with_capacity(longest * 6 + 96);
    serde_json::to_writer(&mut body, request).map_err(std::io::Error::other)?;
    let result = (|| {
        let len = u32::try_from(body.len()).map_err(std::io::Error::other)?;
        conn.write_all(&len.to_ne_bytes())?;
        conn.write_all(&body)?;
        conn.flush()
    })();
    body.iter_mut().for_each(|b| *b = 0);
    std::hint::black_box(&body);
    result
}

/// Lê uma resposta. Um JSON que não é uma resposta conhecida vira erro.
pub fn read_response(conn: &mut impl Read) -> std::io::Result<Response> {
    let mut len = [0u8; 4];
    conn.read_exact(&mut len)?;
    let len = u32::from_ne_bytes(len);
    if len > MAX_MESSAGE {
        return Err(std::io::Error::other(format!("greetd: message too long ({len} bytes)")));
    }
    let mut body = vec![0u8; len as usize];
    conn.read_exact(&mut body)?;
    let value: serde_json::Value = serde_json::from_slice(&body).map_err(std::io::Error::other)?;
    parse_response(&value).ok_or_else(|| std::io::Error::other("greetd: unknown response"))
}

fn parse_response(value: &serde_json::Value) -> Option<Response> {
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
    match value.get("type")?.as_str()? {
        "success" => Some(Response::Success),
        "error" => Some(Response::Error { auth: text("error_type") == "auth_error", description: text("description") }),
        "auth_message" => Some(Response::AuthMessage { kind: text("auth_message_type"), text: text("auth_message") }),
        _ => None,
    }
}

/// Onde a conversa parou.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// O PAM pergunta algo: a senha (`secret`) ou um texto que aparece (`visible`, como um código)
    Prompt { secret: bool, text: String, notes: Vec<String> },
    /// Autenticado: falta o `start`
    Ready { notes: Vec<String> },
    /// Não deu; a sessão já foi cancelada no greetd. `auth`: a senha (ou o usuário) estava errada
    Failed { auth: bool, text: String, notes: Vec<String> },
}

/// A conversa com o greetd, sobre o socket do `$GREETD_SOCK` (ou um par de sockets nos testes).
pub struct Client<C: Read + Write> {
    conn: C,
}

impl<C: Read + Write> Client<C> {
    pub fn new(conn: C) -> Self {
        Client { conn }
    }

    /// Começa o login de um usuário.
    pub fn create_session(&mut self, username: &str) -> std::io::Result<Step> {
        write_message(&mut self.conn, &Request::CreateSession { username })?;
        self.follow()
    }

    /// Responde à pergunta do [`Step::Prompt`]. A senha vai como texto no JSON: quem chama zera a dele.
    pub fn respond(&mut self, answer: &str) -> std::io::Result<Step> {
        write_message(&mut self.conn, &Request::PostAuthMessageResponse { response: Some(answer) })?;
        self.follow()
    }

    /// Desiste do login em andamento (trocar de usuário, por exemplo). Um erro do greetd aqui (não havia
    /// sessão) não importa.
    pub fn cancel(&mut self) -> std::io::Result<()> {
        write_message(&mut self.conn, &Request::CancelSession)?;
        read_response(&mut self.conn).map(|_| ())
    }

    /// Abre a sessão autenticada. Depois do sucesso, a tela deve sair: o greetd a encerra e abre a
    /// sessão no lugar dela.
    pub fn start(&mut self, cmd: &[String], env: &[String]) -> Result<(), String> {
        write_message(&mut self.conn, &Request::StartSession { cmd, env }).map_err(|e| e.to_string())?;
        match read_response(&mut self.conn).map_err(|e| e.to_string())? {
            Response::Success => Ok(()),
            Response::Error { description, .. } => Err(description),
            Response::AuthMessage { text, .. } => Err(text),
        }
    }

    /// Segue a conversa até uma pergunta, o sucesso ou um erro. As mensagens info/error do PAM são
    /// guardadas e confirmadas com uma resposta vazia, como o greetd espera.
    fn follow(&mut self) -> std::io::Result<Step> {
        let mut notes = Vec::new();
        loop {
            match read_response(&mut self.conn)? {
                Response::Success => return Ok(Step::Ready { notes }),
                Response::Error { auth, description } => {
                    // O greetd não descarta a sessão sozinho; sem o cancel, o próximo create_session falha
                    let _ = self.cancel();
                    return Ok(Step::Failed { auth, text: description, notes });
                }
                Response::AuthMessage { kind, text } => match kind.as_str() {
                    "secret" | "visible" => return Ok(Step::Prompt { secret: kind == "secret", text, notes }),
                    _ => {
                        if !text.trim().is_empty() {
                            notes.push(text);
                        }
                        write_message(&mut self.conn, &Request::PostAuthMessageResponse { response: None })?;
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    /// Um greetd de mentira: lê os pedidos e devolve as respostas da lista, na ordem. Devolve os pedidos.
    fn fake(conn: UnixStream, replies: Vec<serde_json::Value>) -> std::thread::JoinHandle<Vec<serde_json::Value>> {
        std::thread::spawn(move || {
            let mut conn = conn;
            let mut seen = Vec::new();
            for reply in replies {
                let mut len = [0u8; 4];
                if conn.read_exact(&mut len).is_err() {
                    break;
                }
                let mut body = vec![0u8; u32::from_ne_bytes(len) as usize];
                conn.read_exact(&mut body).unwrap();
                seen.push(serde_json::from_slice(&body).unwrap());
                let out = serde_json::to_vec(&reply).unwrap();
                conn.write_all(&(out.len() as u32).to_ne_bytes()).unwrap();
                conn.write_all(&out).unwrap();
            }
            seen
        })
    }

    fn secret(text: &str) -> serde_json::Value {
        serde_json::json!({"type": "auth_message", "auth_message_type": "secret", "auth_message": text})
    }

    #[test]
    fn a_good_password_starts_the_session() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let greetd = fake(theirs, vec![secret("Password: "), serde_json::json!({"type": "success"}), serde_json::json!({"type": "success"})]);
        let mut client = Client::new(ours);
        assert_eq!(client.create_session("ana").unwrap(), Step::Prompt { secret: true, text: "Password: ".into(), notes: vec![] });
        assert_eq!(client.respond("s3nha").unwrap(), Step::Ready { notes: vec![] });
        let cmd = vec!["/usr/lib/swaydesk/sway-session".to_string(), "--package".to_string()];
        let env = vec!["XDG_SESSION_TYPE=wayland".to_string()];
        assert_eq!(client.start(&cmd, &env), Ok(()));
        drop(client);
        let seen = greetd.join().unwrap();
        assert_eq!(seen[0], serde_json::json!({"type": "create_session", "username": "ana"}));
        assert_eq!(seen[1], serde_json::json!({"type": "post_auth_message_response", "response": "s3nha"}));
        assert_eq!(seen[2], serde_json::json!({"type": "start_session", "cmd": cmd, "env": env}));
    }

    #[test]
    fn a_wrong_password_cancels_for_the_next_try() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let greetd = fake(
            theirs,
            vec![
                secret("Password: "),
                serde_json::json!({"type": "error", "error_type": "auth_error", "description": "pam_authenticate: AUTH_ERR"}),
                serde_json::json!({"type": "success"}),
                secret("Password: "),
            ],
        );
        let mut client = Client::new(ours);
        client.create_session("ana").unwrap();
        assert_eq!(
            client.respond("errada").unwrap(),
            Step::Failed { auth: true, text: "pam_authenticate: AUTH_ERR".into(), notes: vec![] }
        );
        assert!(matches!(client.create_session("ana").unwrap(), Step::Prompt { secret: true, .. }));
        drop(client);
        let seen = greetd.join().unwrap();
        assert_eq!(seen[2], serde_json::json!({"type": "cancel_session"}));
        assert_eq!(seen[3]["type"], "create_session");
    }

    #[test]
    fn info_messages_are_kept_and_acknowledged() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let greetd = fake(
            theirs,
            vec![
                serde_json::json!({"type": "auth_message", "auth_message_type": "info", "auth_message": "Your password expires in 3 days"}),
                serde_json::json!({"type": "auth_message", "auth_message_type": "error", "auth_message": "  "}),
                serde_json::json!({"type": "auth_message", "auth_message_type": "visible", "auth_message": "Code: "}),
            ],
        );
        let mut client = Client::new(ours);
        assert_eq!(
            client.create_session("ana").unwrap(),
            Step::Prompt { secret: false, text: "Code: ".into(), notes: vec!["Your password expires in 3 days".into()] }
        );
        drop(client);
        let seen = greetd.join().unwrap();
        assert_eq!(seen[1], serde_json::json!({"type": "post_auth_message_response", "response": null}));
        assert_eq!(seen[2], serde_json::json!({"type": "post_auth_message_response", "response": null}));
    }

    #[test]
    fn a_closed_socket_is_an_error() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        drop(theirs);
        assert!(Client::new(ours).create_session("ana").is_err());
    }

    #[test]
    fn a_huge_or_unknown_reply_is_refused() {
        let mut huge: &[u8] = &(MAX_MESSAGE + 1).to_ne_bytes();
        assert!(read_response(&mut huge).is_err());
        let body = br#"{"type":"surprise"}"#;
        let mut bytes = (body.len() as u32).to_ne_bytes().to_vec();
        bytes.extend_from_slice(body);
        assert!(read_response(&mut bytes.as_slice()).is_err());
    }

    #[test]
    fn a_start_error_is_reported() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let greetd = fake(theirs, vec![serde_json::json!({"type": "error", "error_type": "error", "description": "no session"})]);
        assert_eq!(Client::new(ours).start(&["sway".into()], &[]), Err("no session".into()));
        greetd.join().unwrap();
    }
}
