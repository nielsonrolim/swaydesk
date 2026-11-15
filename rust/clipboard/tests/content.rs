//! O read_content em Rust contra o do Python, caso a caso, no corpus do gen_content_corpus.py.
use serde_json::Value;
use swaydesk_clipboard::read_content;

fn b64(text: &str) -> Vec<u8> {
    let table = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let clean: Vec<u8> = text.bytes().filter(|c| *c != b'=').map(table).collect();
    clean.chunks(4).flat_map(|q| {
        let v = q.iter().enumerate().fold(0u32, |acc, (i, x)| acc | (u32::from(*x) << (18 - 6 * i)));
        let bytes = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
        bytes[..q.len().saturating_sub(1)].to_vec()
    }).collect()
}

#[test]
fn matches_python_on_the_corpus() {
    let corpus: Value = serde_json::from_str(include_str!("content_corpus.json")).unwrap();
    let mut failures = Vec::new();
    for case in corpus["cases"].as_array().unwrap() {
        let data = b64(case["input"].as_str().unwrap());
        let got = read_content(&data);
        let expected = (case["kind"].as_str().unwrap(), case["text"].as_str(), case["label"].as_str(), case["utf8"].as_str());
        if (got.kind.as_str(), got.text.as_deref(), got.label.as_deref(), got.utf8.as_deref()) != expected {
            failures.push(format!("{:?}\n  python: {expected:?}\n  rust:   {:?}", &data[..data.len().min(40)], (got.kind, got.text, got.label, got.utf8)));
        }
    }
    let total = corpus["cases"].as_array().unwrap().len();
    assert!(failures.is_empty(), "{} de {total} diferem:\n{}", failures.len(), failures.iter().take(10).cloned().collect::<Vec<_>>().join("\n"));
}
