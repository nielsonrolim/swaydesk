//! O corpo: os casos do tests/test_notifications.py e a conferência com o Python num corpus.
use swaydesk_notifd_logic::{clean_body, MAX_BODY};

#[test]
fn keeps_only_b_i_u() {
    assert_eq!(clean_body("<b>a</b> <i>b</i> <u>c</u>"), "<b>a</b> <i>b</i> <u>c</u>");
}

#[test]
fn link_keeps_only_text() {
    assert_eq!(clean_body(r#"<a href="http://x">site</a>"#), "site");
}

#[test]
fn img_becomes_alt_text() {
    assert_eq!(clean_body(r#"<img src="/etc/passwd" alt="foto"/>"#), "foto");
}

#[test]
fn unknown_tag_is_shown_as_text() {
    assert_eq!(clean_body(r#"<span foreground="red">x</span>"#), r#"&lt;span foreground="red"&gt;x&lt;/span&gt;"#);
}

#[test]
fn attributes_of_allowed_tags_are_dropped() {
    assert_eq!(clean_body(r#"<b size="99999">x</b>"#), "<b>x</b>");
}

#[test]
fn comparison_and_entities_stay_valid() {
    assert_eq!(clean_body("1 < 2 & 3 &amp; 4"), "1 &lt; 2 &amp; 3 &amp; 4");
}

#[test]
fn unbalanced_tags_are_closed() {
    assert_eq!(clean_body("<b><i>x"), "<b><i>x</i></b>");
}

#[test]
fn crossed_tags_nest_correctly() {
    assert_eq!(clean_body("<b><i>x</b>y</i>"), "<b><i>x</i></b>y");
}

#[test]
fn unopened_close_is_ignored() {
    assert_eq!(clean_body("x</b>"), "x");
}

#[test]
fn tag_cut_at_the_end_is_kept_as_text() {
    assert_eq!(clean_body("a <b"), "a &lt;b");
}

#[test]
fn control_characters_are_removed() {
    assert_eq!(clean_body("a\x01b\nc"), "ab\nc");
}

#[test]
fn br_is_a_line_break() {
    assert_eq!(clean_body("a<br/>b"), "a\nb");
}

#[test]
fn body_is_cut() {
    assert!(clean_body(&"x".repeat(MAX_BODY * 2)).chars().count() <= MAX_BODY);
}

/// Saída que o Pango aceita: só b, i e u, bem aninhadas, e nenhum '<' ou '&' solto.
fn assert_valid_markup(text: &str, input: &str) {
    let mut open = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(['<', '&', '>']) {
        let tail = &rest[at..];
        if let Some(after) = tail.strip_prefix('&') {
            let ok = ["&amp;", "&lt;", "&gt;"].iter().any(|e| tail.starts_with(e));
            assert!(ok, "'&' solto em {text:?} (de {input:?})");
            rest = after;
        } else if tail.starts_with('>') {
            panic!("'>' solto em {text:?} (de {input:?})");
        } else {
            let end = tail.find('>').unwrap_or_else(|| panic!("marca sem fim em {text:?} (de {input:?})"));
            let tag = &tail[1..end];
            match tag.strip_prefix('/') {
                Some(name) => assert_eq!(open.pop(), Some(name), "fechamento fora de ordem em {text:?}"),
                None => {
                    assert!(["b", "i", "u"].contains(&tag), "marca {tag:?} em {text:?} (de {input:?})");
                    open.push(tag);
                }
            }
            rest = &tail[end + 1..];
        }
    }
    assert!(open.is_empty(), "marca aberta em {text:?}");
}

#[test]
fn matches_python_on_the_corpus() {
    let corpus: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("body_corpus.json")).expect("corpus em JSON");
    assert!(corpus.len() > 2000);
    let mut wrong = Vec::new();
    for case in &corpus {
        let input = case["input"].as_str().unwrap();
        let expected = case["output"].as_str().unwrap();
        let got = clean_body(input);
        assert_valid_markup(&got, input);
        if got != expected {
            wrong.push(format!("{input:?}\n  python: {expected:?}\n  rust:   {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{} diferenças:\n{}", wrong.len(), wrong.iter().take(15).cloned().collect::<Vec<_>>().join("\n"));
}
