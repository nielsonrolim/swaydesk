use super::*;
use serde_json::json;

const XML: &str = r#"<xkbConfigRegistry>
  <layoutList>
    <layout>
      <configItem>
        <name>us</name>
        <shortDescription>en</shortDescription>
        <description>English (US)</description>
      </configItem>
      <variantList>
        <variant><configItem><name>intl</name><description>English (US, intl., with dead keys)</description></configItem></variant>
        <variant><configItem><name>dvorak</name><description>English (Dvorak)</description></configItem></variant>
      </variantList>
    </layout>
    <layout>
      <configItem>
        <name>br</name>
        <!-- Keyboard indicator for Portuguese layouts -->
        <shortDescription>pt</shortDescription>
        <description>Portuguese (Brazil)</description>
      </configItem>
    </layout>
    <layout>
      <configItem><name>x</name><description>A &amp; B &lt;key&gt;</description></configItem>
    </layout>
  </layoutList>
</xkbConfigRegistry>"#;

fn kb(id: &str, layouts: &[&str], index: Option<u64>) -> serde_json::Value {
    json!({
        "identifier": id, "name": id, "type": "keyboard",
        "xkb_layout_names": layouts,
        "xkb_active_layout_index": index,
        "xkb_active_layout_name": index.and_then(|i| layouts.get(i as usize)).copied(),
    })
}

/// O teclado virtual do wtype, como o Sway o lista: layout nulo, sem nomes.
fn virtual_kb() -> serde_json::Value {
    json!({
        "identifier": "0:0:wlr_virtual_keyboard_v1", "name": "wlr_virtual_keyboard_v1", "type": "keyboard",
        "xkb_layout_names": [], "xkb_active_layout_index": null, "xkb_active_layout_name": null,
    })
}

fn shown_of(inputs: serde_json::Value) -> Option<Shown> {
    shown(&keyboards(&inputs), &registry(XML))
}

#[test]
fn registry_maps_layouts_and_variants_to_the_short_name() {
    let r = registry(XML);
    assert_eq!(r["English (US)"], "us");
    assert_eq!(r["English (US, intl., with dead keys)"], "us");
    assert_eq!(r["English (Dvorak)"], "us");
    assert_eq!(r["Portuguese (Brazil)"], "br");
    assert_eq!(r["A & B <key>"], "x");
    assert_eq!(r.len(), 5);
}

#[test]
fn a_single_layout_hides_the_module() {
    assert_eq!(shown_of(json!([kb("1:1:AT_Translated_Set_2_keyboard", &["Portuguese (Brazil)"], Some(0))])), None);
}

#[test]
fn a_virtual_keyboard_does_not_count_and_leaves_no_gap() {
    // O bug da Waybar: o layout nulo do teclado virtual virava um segundo layout, vazio
    let inputs = json!([kb("1:1:AT_Translated_Set_2_keyboard", &["Portuguese (Brazil)"], Some(0)), virtual_kb()]);
    assert_eq!(shown_of(inputs), None);
    assert_eq!(keyboards(&json!([virtual_kb()])), vec![]);
    // Mesmo um teclado virtual com layouts próprios não conta
    assert_eq!(shown_of(json!([kb("0:0:wlr_virtual_keyboard_v1", &["English (US)", "Portuguese (Brazil)"], Some(0))])), None);
}

#[test]
fn a_real_keyboard_with_virtual_in_its_name_counts() {
    // O teclado USB de um KVM ou de uma VM: vendor:produto de verdade
    let inputs = json!([kb("1267:12:Virtual_USB_Keyboard", &["Portuguese (Brazil)", "English (US)"], Some(0))]);
    assert_eq!(shown_of(inputs).unwrap().short, "br");
    assert!(!is_virtual("1267:12:Virtual_USB_Keyboard", "Virtual USB Keyboard"));
    assert!(is_virtual("0:0:wlr_virtual_keyboard_v1", "wlr_virtual_keyboard_v1"));
}

#[test]
fn two_layouts_show_the_active_one() {
    let two = |i| json!([kb("1:1:kbd", &["Portuguese (Brazil)", "English (US, intl., with dead keys)"], Some(i)), virtual_kb()]);
    assert_eq!(shown_of(two(0)), Some(Shown { short: "br".into(), full: "Portuguese (Brazil)".into() }));
    assert_eq!(shown_of(two(1)), Some(Shown { short: "us".into(), full: "English (US, intl., with dead keys)".into() }));
}

#[test]
fn the_keyboard_with_most_layouts_wins() {
    let inputs = json!([
        kb("2:2:mouse_keys", &["Portuguese (Brazil)"], Some(0)),
        kb("1:1:kbd", &["Portuguese (Brazil)", "English (US)"], Some(1)),
    ]);
    assert_eq!(shown_of(inputs).unwrap().short, "us");
}

#[test]
fn same_short_names_get_numbers_in_order() {
    let inputs = |i| json!([kb("1:1:kbd", &["English (US)", "Portuguese (Brazil)", "English (Dvorak)"], Some(i))]);
    assert_eq!(shown_of(inputs(0)).unwrap().short, "us1");
    assert_eq!(shown_of(inputs(1)).unwrap().short, "br");
    assert_eq!(shown_of(inputs(2)).unwrap().short, "us2");
}

#[test]
fn the_active_layout_falls_back_to_the_name_then_the_first() {
    let mut k = kb("1:1:kbd", &["Portuguese (Brazil)", "English (US)"], Some(9));
    k["xkb_active_layout_name"] = json!("English (US)");
    assert_eq!(shown_of(json!([k.clone()])).unwrap().short, "us");
    k["xkb_active_layout_name"] = json!(null);
    assert_eq!(shown_of(json!([k])).unwrap().short, "br");
}

#[test]
fn an_unknown_layout_gets_a_short_name_from_its_own() {
    assert_eq!(shown_of(json!([kb("1:1:kbd", &["Klingon (pIqaD)", "English (US)"], Some(0))])).unwrap().short, "kli");
}

#[test]
fn other_inputs_and_bad_json_are_ignored() {
    assert_eq!(keyboards(&json!({"not": "a list"})), vec![]);
    assert_eq!(keyboards(&json!([{"type": "pointer", "identifier": "2:2:mouse"}])), vec![]);
    assert_eq!(keyboards(&json!([{"type": "keyboard", "identifier": "3:3:k", "xkb_layout_names": null}])), vec![]);
}

#[test]
fn the_line_hides_with_empty_text() {
    let hidden: serde_json::Value = serde_json::from_str(&line(None)).unwrap();
    assert_eq!(hidden["text"], "");
    let shown = Shown { short: "br".into(), full: "Portuguese (Brazil)".into() };
    let v: serde_json::Value = serde_json::from_str(&line(Some(&shown))).unwrap();
    assert_eq!((v["text"].as_str(), v["tooltip"].as_str(), v["class"].as_str()), (Some("br"), Some("Portuguese (Brazil)"), Some("br")));
}

#[test]
fn ipc_messages_have_the_i3_header() {
    let m = ipc::message(ipc::SUBSCRIBE, br#"["input"]"#);
    assert_eq!(&m[..6], b"i3-ipc");
    assert_eq!(u32::from_ne_bytes(m[6..10].try_into().unwrap()), 9);
    assert_eq!(u32::from_ne_bytes(m[10..14].try_into().unwrap()), 2);
    assert_eq!(&m[14..], br#"["input"]"#);
}
