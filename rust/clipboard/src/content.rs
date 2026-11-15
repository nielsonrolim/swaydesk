//! O tipo de um item pelo conteúdo inteiro: o porte do `read_content` e das funções em volta, de
//! `home/.local/bin/waybar_clipboard.py`. A prévia do cliphist 0.5 não basta: um binário que não é imagem
//! chega como os primeiros 100 bytes, sem marca nenhuma.
//!
//! Fiel ao Python nos detalhes que decidem: o `str.isspace` (os controles 0x1C–0x1F contam como espaço),
//! as categorias gerais do Unicode, o `errors="replace"` e o `errors="ignore"` do UTF-8, e os codecs com
//! BOM. Um corpus gerado pelo próprio Python (gen_content_corpus.py, com o Python 3.12 do Mint 22, de
//! Unicode 15 como o crate de categorias) confere as duas versões. No Python 3.13/3.14 (Debian 13, Ubuntu
//! 26.04), os caracteres do Unicode 15.1/16 contam como atribuídos; aqui, como "sem nome". O limiar de 2%
//! do `readable` absorve a diferença.
use unicode_general_category::{get_general_category, GeneralCategory};

/// Assinaturas de binários comuns. Só nomeiam a linha: o MIME do wl-copy continua sem palpite.
const BINARY_LABELS: [(&[u8], &str); 3] = [(b"%PDF-", "PDF"), (b"PK\x03\x04", "ZIP"), (b"\x7fELF", "ELF")];
/// O bastante para decidir; um item de 10 MB não precisa ser lido todo
pub const CHECK_BYTES: usize = 65536;

/// O que o conteúdo inteiro diz de um item que a prévia deu como texto. `text` é para mostrar e buscar;
/// `utf8`, o texto para copiar no lugar dos bytes, só quando a conversão é certa e sem perda.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Content {
    pub kind: String,
    pub text: Option<String>,
    pub label: Option<String>,
    pub utf8: Option<String>,
}

impl Content {
    fn text(text: String, utf8: bool) -> Content {
        Content { kind: crate::TEXT.into(), utf8: utf8.then(|| text.clone()), text: Some(text), label: None }
    }
}

/// O `str.isspace` do Python: o White_Space do Unicode mais os separadores 0x1C–0x1F.
pub fn py_isspace(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

fn category_bad(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::Control | GeneralCategory::PrivateUse | GeneralCategory::Unassigned | GeneralCategory::Surrogate
    )
}

/// Os primeiros `n` caracteres (o `text[:n]` do Python).
fn chars_prefix(text: &str, n: usize) -> &str {
    match text.char_indices().nth(n) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

/// Texto de verdade quase não tem caractere de controle, de uso privado ou sem nome. O ESC das cores de
/// um terminal conta como texto.
pub fn readable(text: &str) -> bool {
    let (mut bad, mut len) = (0usize, 0usize);
    for c in text.chars() {
        len += 1;
        if category_bad(c) && !py_isspace(c) && c != '\u{1b}' {
            bad += 1;
        }
    }
    bad as f64 <= 0.02 * len as f64
}

/// Mais de um quinto de U+FFFD ou de controles: bytes aleatórios, não um texto em Latin-1.
pub fn garbled(text: &str) -> bool {
    let (mut bad, mut len) = (0usize, 0usize);
    for c in text.chars() {
        len += 1;
        if c == '\u{fffd}' || (get_general_category(c) == GeneralCategory::Control && !py_isspace(c) && c != '\u{1b}') {
            bad += 1;
        }
    }
    bad as f64 > 0.2 * len as f64
}

pub fn valid_utf8(data: &[u8]) -> bool {
    std::str::from_utf8(data).is_ok()
}

/// O `decode("utf-8", errors="ignore")`: os trechos inválidos somem.
fn utf8_ignore(data: &[u8]) -> String {
    data.utf8_chunks().map(|chunk| chunk.valid()).collect()
}

/// Os bytes sem o terminador: um zero solto no fim (tamanho ímpar) e os U+0000 finais.
pub fn utf16_body(data: &[u8]) -> &[u8] {
    let mut data = data;
    if data.len() % 2 == 1 && data.last() == Some(&0) {
        data = &data[..data.len() - 1];
    }
    while data.ends_with(b"\x00\x00") {
        data = &data[..data.len() - 2];
    }
    data
}

fn units_le(data: &[u8]) -> impl Iterator<Item = u16> + '_ {
    data.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]]))
}

/// UTF-16LE com cada surrogate sem par trocado por U+FFFD (o errors="replace" do Python).
fn utf16le_replace(data: &[u8]) -> String {
    char::decode_utf16(units_le(data)).map(|r| r.unwrap_or('\u{fffd}')).collect()
}

fn utf16le_strict(data: &[u8]) -> Option<String> {
    if data.len() % 2 == 1 {
        return None;
    }
    char::decode_utf16(units_le(data)).collect::<Result<String, _>>().ok()
}

/// O texto, se os bytes forem UTF-16LE sem BOM legível (links copiados de alguns apps).
pub fn utf16le_text(data: &[u8]) -> Option<String> {
    let data = utf16_body(data);
    if data.is_empty() || data.len() % 2 == 1 {
        return None;
    }
    let sample = utf16le_replace(&data[..data.len().min(CHECK_BYTES)]);
    if sample.contains('\0') || sample.contains('\u{fffd}') || !readable(&sample) {
        return None;
    }
    utf16le_strict(data) // um surrogate sem par depois da amostra: None
}

/// O padrão que dá certeza do UTF-16LE: zero em quase todas as posições ímpares e em nenhuma par.
pub fn latin_utf16(data: &[u8]) -> bool {
    let odd: Vec<u8> = data.iter().take(CHECK_BYTES).skip(1).step_by(2).copied().collect();
    let zeros = odd.iter().filter(|b| **b == 0).count();
    zeros as f64 >= 0.75 * odd.len() as f64 && !data.iter().step_by(2).any(|b| *b == 0)
}

/// Decodifica com o codec do Python ("utf-16" ou "utf-32", que leem o BOM e o tiram); None num erro.
fn decode_bom(data: &[u8], codec: &str) -> Option<String> {
    match codec {
        "utf-32" => {
            if data.len() % 4 != 0 {
                return None;
            }
            let (le, rest) = match data.get(..4) {
                Some(b"\xff\xfe\x00\x00") => (true, &data[4..]),
                Some(b"\x00\x00\xfe\xff") => (false, &data[4..]),
                _ => (true, data),
            };
            rest.chunks_exact(4)
                .map(|q| {
                    let v = if le { u32::from_le_bytes([q[0], q[1], q[2], q[3]]) } else { u32::from_be_bytes([q[0], q[1], q[2], q[3]]) };
                    char::from_u32(v)
                })
                .collect()
        }
        _ => {
            if data.len() % 2 == 1 {
                return None;
            }
            let (le, rest) = match data.get(..2) {
                Some(b"\xff\xfe") => (true, &data[2..]),
                Some(b"\xfe\xff") => (false, &data[2..]),
                _ => (true, data),
            };
            let units = rest.chunks_exact(2).map(|p| if le { u16::from_le_bytes([p[0], p[1]]) } else { u16::from_be_bytes([p[0], p[1]]) });
            char::decode_utf16(units).collect::<Result<String, _>>().ok()
        }
    }
}

/// O texto de um item com BOM. FF FE 00 00 é UTF-32LE ou um UTF-16LE que começa com U+0000: se não for
/// UTF-32 válido, tenta UTF-16. O terminador não conta.
pub fn bom_text(data: &[u8]) -> Option<String> {
    let codecs: &[&str] = if data.starts_with(b"\xff\xfe\x00\x00") || data.starts_with(b"\x00\x00\xfe\xff") { &["utf-32", "utf-16"] } else { &["utf-16"] };
    for codec in codecs {
        let body = if *codec == "utf-32" { data } else { utf16_body(data) };
        if let Some(text) = decode_bom(body, codec) {
            return Some(text.trim_matches('\0').to_string()); // o U+0000 do começo é o resto do BOM ambíguo
        }
    }
    None
}

pub fn binary(data: &[u8]) -> Content {
    Content {
        kind: crate::BINARY.into(),
        label: BINARY_LABELS.iter().find(|(magic, _)| data.starts_with(magic)).map(|(_, name)| name.to_string()),
        ..Default::default()
    }
}

/// O tipo pelo conteúdo inteiro, na ordem do Python: BOM de UTF-32 ou de UTF-16; sem byte zero (ou só o
/// terminador de um texto C), o UTF-8 de sempre; UTF-16LE com o padrão latino; UTF-8 legível com zeros
/// perdidos no meio; UTF-16LE legível sem o padrão (cirílico, CJK), que se mostra mas só se copia
/// convertido com certeza. O resto é binário.
pub fn read_content(data: &[u8]) -> Content {
    if data.starts_with(b"\xff\xfe") || data.starts_with(b"\xfe\xff") || data.starts_with(b"\x00\x00\xfe\xff") {
        return match bom_text(data) {
            Some(text) if readable(chars_prefix(&text, CHECK_BYTES)) => Content::text(text, true),
            _ => binary(data),
        };
    }
    let mut end = data.len();
    while end > 0 && data[end - 1] == 0 {
        end -= 1;
    }
    let body = &data[..end];
    if !body.contains(&0) {
        let text = String::from_utf8_lossy(body).into_owned();
        // Curto: na dúvida, texto
        if body.len() >= 16 && !valid_utf8(body) && garbled(chars_prefix(&text, CHECK_BYTES)) {
            return binary(data); // bytes aleatórios sem zero: só para mostrar, a cópia leva os bytes
        }
        return Content::text(text, false);
    }
    let text = utf16le_text(data);
    if let Some(t) = &text {
        if latin_utf16(utf16_body(data)) {
            return Content::text(t.clone(), true);
        }
    }
    let stripped: Vec<u8> = body.iter().copied().filter(|b| *b != 0).collect();
    if valid_utf8(&stripped) && readable(&utf8_ignore(&stripped[..stripped.len().min(CHECK_BYTES)])) {
        return Content::text(String::from_utf8(stripped).unwrap_or_default(), false); // zeros perdidos no meio
    }
    let Some(text) = text else { return binary(data) };
    // Só se converte na cópia com todo byte zero numa posição ímpar, do começo ao fim
    let certain = !utf16_body(data).iter().step_by(2).any(|b| *b == 0) && !text.contains('\0') && readable(&text);
    Content::text(text, certain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn t(text: &str, utf8: bool) -> Content {
        Content::text(text.into(), utf8)
    }

    // Os testes de ReadContentTests (test_cliphist_parse.py)
    #[test]
    fn latin_and_non_latin_utf16_are_text_and_copied_as_utf8() {
        for text in ["https://x.com/a", " https://x.com/a", "it’s ‘quoted’ — text and more", "https://exemplo.com/ação", "中文 テキスト", "Привет мир", "👨\u{200d}👩\u{200d}👧 ok"] {
            assert_eq!(read_content(&utf16le(text)), t(text, true), "{text}");
        }
        let long = format!("https://x.com/{}", "Привет".repeat(50));
        assert_eq!(read_content(&utf16le(&long)), t(&long, true));
        for tail in [&b"\x00\x00"[..], &b"\x00"[..]] {
            assert_eq!(read_content(&[utf16le("hello"), tail.to_vec()].concat()), t("hello", true));
        }
    }

    #[test]
    fn boms() {
        let be: Vec<u8> = [&b"\xfe\xff"[..], &"中文 ok".encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<_>>()].concat();
        let le: Vec<u8> = [&b"\xff\xfe"[..], &utf16le("中文 ok")].concat();
        let u32le: Vec<u8> = [&b"\xff\xfe\x00\x00"[..], &"中文 ok".chars().flat_map(|c| (c as u32).to_le_bytes()).collect::<Vec<_>>()].concat();
        for data in [be, le, u32le] {
            assert_eq!(read_content(&data), t("中文 ok", true));
        }
        assert_eq!(read_content(b"\xff\xfe\x00\xd8\x00\x00lixo").kind, crate::BINARY);
        assert_eq!(read_content(&[&b"\xff\xfe"[..], &utf16le("hello"), b"\x00"].concat()), t("hello", true));
        assert_eq!(read_content(&[&b"\xff\xfe\x00\x00"[..], &utf16le("hi")].concat()), t("hi", true));
    }

    #[test]
    fn text_without_zero_bytes_is_copied_as_is() {
        for data in [&b"\x1b[31mred\x1b[0m"[..], b"a\xe7\xe3o", "olá 😀".as_bytes(), b"%PDF-1.4 sobre pdf"] {
            let c = read_content(data);
            assert_eq!((c.kind.as_str(), c.utf8), (crate::TEXT, None));
        }
        assert_eq!(read_content(b"\xc7\xc3\xd5").kind, crate::TEXT); // curto: na dúvida, texto
    }

    #[test]
    fn zeros() {
        let text = "一二三 中文"; // 一 é U+4E00: o byte zero vem na posição par
        assert_eq!(read_content(&utf16le(text)), t(text, false));
        assert_eq!(read_content(b"\x1b[31mred\x1b[0m\x00abc"), t("\x1b[31mred\x1b[0mabc", false));
        assert!(read_content(&[0x89, 1, 0, 0x41, 0x42, 0x43, 0xe9, 0x20].repeat(4)).utf8.is_none());
        assert_eq!(read_content(b"abc\x00de"), t("abcde", false));
        assert_eq!(read_content(b"hello\x00"), t("hello", false));
        assert!(read_content(b"\xe9\x00").utf8.is_none());
    }

    #[test]
    fn binaries() {
        assert_eq!(read_content(b"%PDF-1.7\n%\xe2\xe3\n1 0 obj\x00\x01\x02 stream"), Content { kind: crate::BINARY.into(), label: Some("PDF".into()), ..Default::default() });
        assert_eq!(read_content(b"PK\x03\x04\x14\x00\x00\x00\x08\x00\x00\x00!\x00").label.as_deref(), Some("ZIP"));
        let all: Vec<u8> = (0..=255u8).collect::<Vec<_>>().repeat(4);
        assert_eq!(read_content(&all).kind, crate::BINARY);
    }

    #[test]
    fn isspace_like_python() {
        assert!(py_isspace('\u{1c}') && py_isspace('\u{85}') && py_isspace('\u{3000}'));
        assert!(!py_isspace('\u{1b}') && !py_isspace('a'));
    }
}
