//! O corpo da notificação em marcação Pango válida: o porte do `clean_body` de
//! `waybar_notifications.py`, com o `html.parser.HTMLParser` do Python (o do 3.12 com as correções de
//! segurança, como o Ubuntu e o Debian empacotam) e o `html.unescape` portados aqui.
//!
//! O corpo vem de qualquer programa da sessão: só `<b>`, `<i>` e `<u>` continuam marcação, `<br>` vira
//! quebra de linha, `<img>` vira o texto alternativo e `<a>` vira só o texto. O resto aparece como texto,
//! escapado. O analisador recebe o corpo de uma vez, sem o `close()`: o que fica guardado no fim (uma
//! marca incompleta, uma referência cortada, o resto de um `<script>`) volta como texto cru.
//!
//! Tudo trabalha em `char`, como as posições de uma `str` do Python.

use crate::entities::{HTML5, INVALID_CHARREFS, INVALID_CODEPOINTS};

pub const MAX_BODY: usize = 4000;

const ALLOWED: [&str; 3] = ["b", "i", "u"];
const CDATA_CONTENT: [&str; 7] = ["script", "style", "xmp", "iframe", "noembed", "noframes", "plaintext"];
const RCDATA_CONTENT: [&str; 2] = ["textarea", "title"];

/// O `CONTROL` do Python: os caracteres de controle, menos o tab, o \n e o \r.
pub fn is_control(c: char) -> bool {
    matches!(c, '\x00'..='\x08' | '\x0b' | '\x0c' | '\x0e'..='\x1f' | '\x7f'..='\u{9f}')
}

/// `[\t\n\r\f ]`
fn is_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\x0c' | ' ')
}

fn escape_into(text: impl IntoIterator<Item = char>, out: &mut String) {
    for c in text {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

// ---------------------------------------------------------------- html.unescape

fn numeric_ref(digits: &[char], radix: u32) -> String {
    // Os dígitos podem ser muitos: acima de 0x10FFFF tanto faz o valor exato
    let mut value: u64 = 0;
    for c in digits {
        value = (value * radix as u64 + c.to_digit(radix).unwrap_or(0) as u64).min(0x110000);
    }
    let value = value as u32;
    if let Ok(at) = INVALID_CHARREFS.binary_search_by_key(&value, |(k, _)| *k) {
        return INVALID_CHARREFS[at].1.to_string();
    }
    if (0xD800..=0xDFFF).contains(&value) || value > 0x10FFFF {
        return '\u{FFFD}'.to_string();
    }
    if INVALID_CODEPOINTS.binary_search(&value).is_ok() {
        return String::new();
    }
    char::from_u32(value).map(String::from).unwrap_or_default()
}

fn named_ref(name: &str) -> Option<&'static str> {
    HTML5.binary_search_by(|(k, _)| (*k).cmp(name)).ok().map(|at| HTML5[at].1)
}

/// O `html.unescape`: `&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)`.
pub fn unescape(text: &[char]) -> String {
    let mut out = String::with_capacity(text.len());
    let n = text.len();
    let mut i = 0;
    while i < n {
        if text[i] != '&' {
            out.push(text[i]);
            i += 1;
            continue;
        }
        let s = i + 1;
        // Numérica, decimal ou hexadecimal
        if s < n && text[s] == '#' {
            let (start, radix) = if s + 1 < n && matches!(text[s + 1], 'x' | 'X') { (s + 2, 16) } else { (s + 1, 10) };
            let mut e = start;
            while e < n && text[e].is_digit(radix) {
                e += 1;
            }
            if e > start {
                out.push_str(&numeric_ref(&text[start..e], radix));
                i = if e < n && text[e] == ';' { e + 1 } else { e };
                continue;
            }
            // "&#" sem dígitos: o '#' não entra no nome, então não é referência
            out.push('&');
            i += 1;
            continue;
        }
        let mut e = s;
        while e < n && e - s < 32 && !matches!(text[e], '\t' | '\n' | '\x0c' | ' ' | '<' | '&' | '#' | ';') {
            e += 1;
        }
        if e == s {
            out.push('&');
            i += 1;
            continue;
        }
        if e < n && text[e] == ';' {
            e += 1;
        }
        let name: String = text[s..e].iter().collect();
        if let Some(value) = named_ref(&name) {
            out.push_str(value);
            i = e;
            continue;
        }
        // O prefixo mais longo que é referência (com pelo menos 2 caracteres); o resto fica
        let mut found = false;
        let ends: Vec<usize> = name.char_indices().map(|(at, _)| at).collect();
        for x in (2..e - s).rev() {
            if let Some(value) = named_ref(&name[..ends[x]]) {
                out.push_str(value);
                out.extend(&text[s + x..e]);
                found = true;
                break;
            }
        }
        if !found {
            out.push('&');
            out.extend(&text[s..e]);
        }
        i = e;
    }
    out
}

// ---------------------------------------------------------------- os padrões do HTMLParser

/// `[a-zA-Z][^\t\n\r\f />]*`: o fim do nome que começa em `at`.
fn tag_name_end(raw: &[char], at: usize) -> usize {
    let mut j = at + 1;
    while j < raw.len() && !matches!(raw[j], '\t' | '\n' | '\r' | '\x0c' | ' ' | '/' | '>') {
        j += 1;
    }
    j
}

fn attr_name_start_ok(raw: &[char], at: usize) -> bool {
    at > 0
        && matches!(raw[at - 1], '\'' | '"' | '\t' | '\n' | '\r' | '\x0c' | ' ' | '/')
        && at < raw.len()
        && !matches!(raw[at], '\t' | '\n' | '\r' | '\x0c' | ' ' | '/' | '>')
}

/// O nome de um atributo a partir de `at` (já conferido o começo): `[^\t\n\r\f />][^\t\n\r\f /=>]*`.
fn attr_name_end(raw: &[char], at: usize) -> usize {
    let mut j = at + 1;
    while j < raw.len() && !matches!(raw[j], '\t' | '\n' | '\r' | '\x0c' | ' ' | '/' | '=' | '>') {
        j += 1;
    }
    j
}

/// `[\t\n\r\f ]*=[\t\n\r\f ]*('[^']*'|"[^"]*"|(?!['"])[^>\t\n\r\f ]*)`: o fim e onde está o valor.
fn attr_value(raw: &[char], at: usize) -> Option<(usize, usize, usize)> {
    let n = raw.len();
    let mut j = at;
    while j < n && is_space(raw[j]) {
        j += 1;
    }
    if j >= n || raw[j] != '=' {
        return None;
    }
    j += 1;
    let after_eq = j;
    while j < n && is_space(raw[j]) {
        j += 1;
    }
    let vs = j;
    if j < n && (raw[j] == '\'' || raw[j] == '"') {
        let quote = raw[j];
        match raw[j + 1..].iter().position(|c| *c == quote) {
            Some(close) => {
                let ve = j + 1 + close + 1;
                return Some((ve, vs, ve));
            }
            // Aspas sem fim: a expressão regular devolve o último espaço depois do '=' e fica com um
            // valor vazio ali; sem espaço, não há valor
            None if j > after_eq => return Some((j - 1, j - 1, j - 1)),
            None => return None,
        }
    }
    while j < n && !matches!(raw[j], '>' | '\t' | '\n' | '\r' | '\x0c' | ' ') {
        j += 1;
    }
    Some((j, vs, j))
}

/// O `locatetagend` a partir de `at` (a letra do nome): até onde a marca vai.
fn locate_tag_end(raw: &[char], at: usize) -> usize {
    let n = raw.len();
    let mut j = tag_name_end(raw, at);
    while j < n && matches!(raw[j], '\t' | '\n' | '\r' | '\x0c' | ' ' | '/') {
        j += 1;
    }
    while attr_name_start_ok(raw, j) {
        j = attr_name_end(raw, j);
        if let Some((end, _, _)) = attr_value(raw, j) {
            j = end;
        }
        while j < n && matches!(raw[j], '\t' | '\n' | '\r' | '\x0c' | ' ' | '/') {
            j += 1;
        }
    }
    if j < n && raw[j] == '>' {
        j += 1;
    }
    j
}

/// `(?:[\t\n\r\f ]|/(?!>))*`
fn skip_tolerant_space(raw: &[char], mut j: usize) -> usize {
    let n = raw.len();
    while j < n && (is_space(raw[j]) || (raw[j] == '/' && raw.get(j + 1) != Some(&'>'))) {
        j += 1;
    }
    j
}

fn find(raw: &[char], what: char, from: usize) -> Option<usize> {
    raw.get(from..)?.iter().position(|c| *c == what).map(|p| p + from)
}

fn find_str(raw: &[char], what: &str, from: usize) -> Option<usize> {
    let what: Vec<char> = what.chars().collect();
    if from > raw.len() {
        return None;
    }
    raw[from..].windows(what.len()).position(|w| w == what.as_slice()).map(|p| p + from)
}

fn starts_with(raw: &[char], at: usize, what: &str) -> bool {
    (at..).zip(what.chars()).all(|(j, c)| raw.get(j) == Some(&c))
}

fn lower(text: &[char]) -> String {
    text.iter().collect::<String>().to_lowercase()
}

// ---------------------------------------------------------------- o analisador e o _BodyParser

struct Parser {
    out: String,
    open: Vec<&'static str>,
    cdata: Option<String>,
    escapable: bool,
}

impl Parser {
    fn data(&mut self, text: &[char]) {
        escape_into(text.iter().copied(), &mut self.out);
    }

    fn data_str(&mut self, text: &str) {
        escape_into(text.chars(), &mut self.out);
    }

    fn comment(&mut self, text: &[char]) {
        self.data_str("<!--");
        self.data(text);
        self.data_str("-->");
    }

    fn starttag(&mut self, tag: &str, attrs: &[(String, Option<String>)], raw: &[char]) {
        if let Some(allowed) = ALLOWED.iter().find(|t| **t == tag) {
            self.out.push('<');
            self.out.push_str(allowed);
            self.out.push('>');
            self.open.push(allowed);
        } else if tag == "br" {
            self.out.push('\n');
        } else if tag == "img" {
            // A imagem não é buscada: fica o texto alternativo (o último alt, como o dict do Python)
            let alt = attrs.iter().rev().find(|(k, _)| k == "alt").and_then(|(_, v)| v.clone()).unwrap_or_default();
            self.data_str(&alt);
        } else if tag != "a" {
            self.data(raw); // do link fica só o texto; o resto, como texto
        }
    }

    fn startendtag(&mut self, tag: &str, attrs: &[(String, Option<String>)], raw: &[char]) {
        if tag == "br" || tag == "img" {
            self.starttag(tag, attrs, raw);
        } else if !ALLOWED.contains(&tag) && tag != "a" {
            self.data(raw);
        }
    }

    fn endtag(&mut self, tag: &str) {
        if let Some(allowed) = ALLOWED.iter().find(|t| **t == tag) {
            if self.open.contains(allowed) {
                // Fecha o que ficou aberto dentro, para aninhar certo
                while let Some(last) = self.open.pop() {
                    if last == *allowed {
                        break;
                    }
                    self.out.push_str("</");
                    self.out.push_str(last);
                    self.out.push('>');
                }
                self.out.push_str("</");
                self.out.push_str(allowed);
                self.out.push('>');
            }
        } else if !matches!(tag, "a" | "br" | "img") {
            self.data_str(&format!("</{tag}>"));
        }
    }

    /// `parse_starttag`: o fim da marca, ou None se ela não fechou.
    fn parse_starttag(&mut self, raw: &[char], i: usize) -> Option<usize> {
        let endpos = locate_tag_end(raw, i + 1);
        if raw[endpos - 1] != '>' {
            return None;
        }
        let name_end = tag_name_end(raw, i + 1);
        let tag = lower(&raw[i + 1..name_end]);
        let mut k = skip_tolerant_space(raw, name_end);
        let mut attrs = Vec::new();
        while k < endpos {
            // attrfind_tolerant
            if !attr_name_start_ok(raw, k) {
                break;
            }
            let name_end = attr_name_end(raw, k);
            let name = lower(&raw[k..name_end]);
            let mut j = name_end;
            let mut value = None;
            if let Some((end, vs, ve)) = attr_value(raw, name_end) {
                let mut v = &raw[vs..ve];
                if v.len() >= 2 && (v[0] == '\'' || v[0] == '"') && v[v.len() - 1] == v[0] {
                    v = &v[1..v.len() - 1];
                }
                value = Some(if v.is_empty() { String::new() } else { unescape(v) });
                j = end;
            }
            attrs.push((name, value));
            k = skip_tolerant_space(raw, j);
        }
        let end: String = raw[k..endpos].iter().collect();
        let end = end.trim_matches(py_space);
        let text = &raw[i..endpos];
        if end != ">" && end != "/>" {
            self.data(text);
            return Some(endpos);
        }
        if end.ends_with("/>") {
            self.startendtag(&tag, &attrs, text);
        } else {
            self.starttag(&tag, &attrs, text);
            if CDATA_CONTENT.contains(&tag.as_str()) {
                self.cdata = Some(tag);
                self.escapable = false;
            } else if RCDATA_CONTENT.contains(&tag.as_str()) {
                self.cdata = Some(tag);
                self.escapable = true;
            }
        }
        Some(endpos)
    }

    fn bogus_comment(&mut self, raw: &[char], i: usize) -> Option<usize> {
        let pos = find(raw, '>', i + 2)?;
        self.comment(&raw[i + 2..pos]);
        Some(pos + 1)
    }

    fn parse_endtag(&mut self, raw: &[char], i: usize) -> Option<usize> {
        find(raw, '>', i + 2)?;
        if !raw.get(i + 2).is_some_and(char::is_ascii_alphabetic) {
            if raw.get(i + 2) == Some(&'>') {
                return Some(i + 3); // "</>" some
            }
            return self.bogus_comment(raw, i);
        }
        let j = locate_tag_end(raw, i + 2);
        if raw[j - 1] != '>' {
            return None;
        }
        let tag = lower(&raw[i + 2..tag_name_end(raw, i + 2)]);
        self.endtag(&tag);
        self.cdata = None;
        self.escapable = true;
        Some(j)
    }

    fn parse_comment(&mut self, raw: &[char], i: usize) -> Option<usize> {
        // commentclose: --!?> em qualquer ponto; senão commentabruptclose: -?> logo no começo
        let mut j = i + 4;
        while j < raw.len() {
            if starts_with(raw, j, "--!>") {
                self.comment(&raw[i + 4..j]);
                return Some(j + 4);
            }
            if starts_with(raw, j, "-->") {
                self.comment(&raw[i + 4..j]);
                return Some(j + 3);
            }
            j += 1;
        }
        for close in ["->", ">"] {
            if starts_with(raw, i + 4, close) {
                self.comment(&[]);
                return Some(i + 4 + close.chars().count());
            }
        }
        None
    }

    fn parse_html_declaration(&mut self, raw: &[char], i: usize) -> Option<usize> {
        if starts_with(raw, i, "<![CDATA[") {
            let j = find_str(raw, "]]>", i + 9)?;
            self.data_str("<![");
            self.data(&raw[i + 3..j]);
            self.data_str("]>");
            return Some(j + 3);
        }
        if raw.len() >= i + 9 && lower(&raw[i..i + 9]) == "<!doctype" {
            let gt = find(raw, '>', i + 9)?;
            self.data_str("<!");
            self.data(&raw[i + 2..gt]);
            self.data_str(">");
            return Some(gt + 1);
        }
        if starts_with(raw, i, "<![") {
            let j = find(raw, '>', i + 3)?;
            if raw[j - 1] == ']' {
                self.data_str("<![");
                self.data(&raw[i + 3..j - 1]);
                self.data_str("]>");
            } else {
                self.comment(&raw[i + 2..j]);
            }
            return Some(j + 1);
        }
        self.bogus_comment(raw, i)
    }

    /// O `</tag` que fecha o conteúdo cru, seguido de espaço, '/' ou '>' (sem olhar maiúsculas).
    fn cdata_close(&self, raw: &[char], from: usize) -> Option<usize> {
        let tag: Vec<char> = self.cdata.as_ref()?.chars().collect();
        let mut j = from;
        while j + 2 + tag.len() < raw.len() {
            if raw[j] == '<'
                && raw[j + 1] == '/'
                && raw[j + 2..j + 2 + tag.len()].iter().zip(&tag).all(|(a, b)| a.eq_ignore_ascii_case(b))
                && matches!(raw[j + 2 + tag.len()], '\t' | '\n' | '\r' | '\x0c' | ' ' | '/' | '>')
            {
                return Some(j);
            }
            j += 1;
        }
        None
    }

    /// O `goahead(0)`: devolve até onde analisou; o resto fica guardado.
    fn goahead(&mut self, raw: &[char]) -> usize {
        let n = raw.len();
        let mut i = 0;
        while i < n {
            let j = if self.cdata.is_none() {
                match find(raw, '<', i) {
                    Some(j) => j,
                    None => {
                        // Uma referência pode estar cortada no fim: espera (aqui, fica guardada)
                        let from = i.max(n.saturating_sub(34));
                        if let Some(amp) = raw[from..].iter().rposition(|c| *c == '&').map(|p| p + from) {
                            if !raw[amp..].iter().any(|c| matches!(c, '\t' | '\n' | '\r' | '\x0c' | ' ' | ';')) {
                                break;
                            }
                        }
                        n
                    }
                }
            } else if self.cdata.as_deref() == Some("plaintext") {
                n // o plaintext vai até o fim
            } else {
                match self.cdata_close(raw, i) {
                    Some(j) => j,
                    None => break,
                }
            };
            if i < j {
                if self.escapable {
                    let text = unescape(&raw[i..j]);
                    self.data_str(&text);
                } else {
                    self.data(&raw[i..j]);
                }
            }
            i = j;
            if i == n {
                break;
            }
            // Aqui raw[i] é '<'
            let k = if raw.get(i + 1).is_some_and(char::is_ascii_alphabetic) {
                self.parse_starttag(raw, i)
            } else if starts_with(raw, i, "</") {
                self.parse_endtag(raw, i)
            } else if starts_with(raw, i, "<!--") {
                self.parse_comment(raw, i)
            } else if starts_with(raw, i, "<?") {
                find(raw, '>', i + 2).map(|j| {
                    self.data_str("<?");
                    self.data(&raw[i + 2..j]);
                    self.data_str(">");
                    j + 1
                })
            } else if starts_with(raw, i, "<!") {
                self.parse_html_declaration(raw, i)
            } else if i + 1 < n {
                self.data_str("<");
                Some(i + 1)
            } else {
                None
            };
            match k {
                Some(k) => i = k,
                None => break,
            }
        }
        i
    }

    fn result(mut self) -> String {
        while let Some(tag) = self.open.pop() {
            self.out.push_str("</");
            self.out.push_str(tag);
            self.out.push('>');
        }
        self.out
    }
}

/// O `str.isspace` do Python, para o `strip()`.
fn py_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\x1c'..='\x1f')
}

/// Marcação Pango válida a partir do corpo recebido.
pub fn clean_body(text: &str) -> String {
    let raw: Vec<char> = text.chars().take(MAX_BODY).filter(|c| !is_control(*c)).collect();
    let mut parser = Parser { out: String::new(), open: Vec::new(), cdata: None, escapable: true };
    let done = parser.goahead(&raw);
    // O que ficou guardado no analisador (como "a <b") volta como texto
    parser.data(&raw[done..]);
    parser.result().trim_matches(py_space).to_string()
}
