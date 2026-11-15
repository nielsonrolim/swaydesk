//! O estado do áudio lido do `pw-dump`, sem GTK: o porte do `read_dump`, `read_state`, `cubic_to_percent`
//! e `route_volume` de `home/.local/bin/waybar-audio`. (O JSON do `pactl` quebra com textos acentuados.)
use std::collections::HashMap;

use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Sink,
    Source,
}

impl Kind {
    fn direction(self) -> &'static str {
        match self {
            Kind::Sink => "Output",
            Kind::Source => "Input",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Kind::Sink => "sink",
            Kind::Source => "source",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: i64,
    pub kind: Kind,
    pub name: String,
    pub desc: String,
    device: Option<i64>,
    profile_dev: Option<Value>,
    pub mute: bool,
    pub vol: i64,
}

/// Uma linha da lista: uma saída ou entrada, ou uma porta dela numa placa com várias.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub node: Node,
    pub port: Option<String>,
    pub title: String,
    pub sub: String,
    pub active: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct State {
    pub sinks: Vec<Entry>,
    pub sources: Vec<Entry>,
    pub current_sink: Option<Node>,
    pub current_source: Option<Node>,
}

/// Os objetos do `pw-dump`, com as mudanças que chegaram durante a leitura já aplicadas: a saída pode
/// trazer mais de um array JSON em sequência; o último estado de cada id vale, um objeto sem `type`
/// remove o id, e os metadados em atualização trazem só as chaves alteradas.
pub fn parse_dump(text: &str) -> Result<Vec<Value>, String> {
    let mut objects: Vec<(i64, Value)> = Vec::new(); // na ordem de chegada, como o dict do Python
    let index = |objects: &Vec<(i64, Value)>, id: i64| objects.iter().position(|(i, _)| *i == id);
    for chunk in serde_json::Deserializer::from_str(text).into_iter::<Value>() {
        let chunk = chunk.map_err(|e| e.to_string())?;
        for mut o in chunk.as_array().cloned().unwrap_or_default() {
            let Some(id) = o.get("id").and_then(Value::as_i64) else { continue };
            let found = index(&objects, id);
            match o.get("type").and_then(Value::as_str) {
                None => {
                    if let Some(i) = found {
                        objects.remove(i);
                    }
                }
                Some("PipeWire:Interface:Metadata") if found.is_some() => {
                    let i = found.unwrap();
                    let old = objects[i].1.get("metadata").and_then(Value::as_array).cloned().unwrap_or_default();
                    let new = o.get("metadata").and_then(Value::as_array).cloned().unwrap_or_default();
                    let changed: Vec<Option<&Value>> = new.iter().map(|m| m.get("key")).collect();
                    let mut merged: Vec<Value> = old.into_iter().filter(|m| !changed.contains(&m.get("key"))).collect();
                    merged.extend(new);
                    o["metadata"] = Value::Array(merged);
                    objects[i].1 = o; // o dict do Python mantém o lugar da chave
                }
                Some(_) => match found {
                    Some(i) => objects[i].1 = o,
                    None => objects.push((id, o)),
                },
            }
        }
    }
    Ok(objects.into_iter().map(|(_, o)| o).collect())
}

/// O PipeWire guarda o volume cúbico; o wpctl mostra a raiz cúbica (`** (1 / 3)` e o `round` do Python).
pub fn cubic_to_percent(volumes: &[f64]) -> i64 {
    let max = volumes.iter().copied().fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.max(v)))).unwrap_or(0.0);
    (max.powf(1.0 / 3.0) * 100.0).round_ties_even() as i64
}

fn floats(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default()
}

fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

struct Device {
    desc: String,
    routes: Vec<Value>,
    active: Vec<Value>,
    profile: Option<Value>,
}

/// Numa placa, o volume e o mudo valem os da porta ativa (o Route do Device): o `wpctl set-volume` muda o
/// Route, e o Props do node pode ficar com o valor antigo (visto no Debian 13, WirePlumber 1.4).
fn route_volume(node: &mut Node, device: Option<&Device>) {
    let Some(device) = device else { return };
    let route = device.active.iter().find(|r| {
        str_of(r, "direction") == node.kind.direction() && r.get("device") == node.profile_dev.as_ref()
    });
    let Some(props) = route.and_then(|r| r.get("props")) else { return };
    let volumes = floats(props.get("channelVolumes"));
    if !volumes.is_empty() {
        node.vol = cubic_to_percent(&volumes);
    }
    if let Some(mute) = props.get("mute") {
        node.mute = truthy(mute);
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Dispositivos, padrão, volume e mudo, a partir dos objetos do `pw-dump`.
pub fn read_state(dump: &[Value]) -> State {
    let empty = Map::new();
    let mut devices: HashMap<i64, Device> = HashMap::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut default: HashMap<&'static str, Option<String>> = HashMap::new();
    for o in dump {
        let kind = str_of(o, "type");
        let info = o.get("info").filter(|v| !v.is_null());
        let props = info.and_then(|i| i.get("props")).and_then(Value::as_object).unwrap_or(&empty);
        let params = info.and_then(|i| i.get("params")).cloned().unwrap_or(Value::Object(Map::new()));
        let prop = |k: &str| props.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let id = o.get("id").and_then(Value::as_i64).unwrap_or(-1);
        if kind == "PipeWire:Interface:Device" {
            devices.insert(id, Device {
                desc: prop("device.description"),
                routes: array(&params, "EnumRoute").to_vec(),
                active: array(&params, "Route").to_vec(),
                profile: array(&params, "Profile").first().and_then(|p| p.get("index")).cloned(),
            });
        } else if kind == "PipeWire:Interface:Node" && matches!(props.get("media.class").and_then(Value::as_str), Some("Audio/Sink" | "Audio/Source")) {
            let p = array(&params, "Props").first().cloned().unwrap_or(Value::Object(Map::new()));
            let mut volumes = floats(p.get("channelVolumes"));
            if volumes.is_empty() {
                volumes = vec![p.get("volume").and_then(Value::as_f64).unwrap_or(0.0)];
            }
            let name = prop("node.name");
            nodes.push(Node {
                id,
                kind: if props.get("media.class").and_then(Value::as_str) == Some("Audio/Sink") { Kind::Sink } else { Kind::Source },
                desc: props.get("node.description").and_then(Value::as_str).map(String::from).unwrap_or_else(|| name.clone()),
                name,
                device: props.get("device.id").and_then(Value::as_i64),
                profile_dev: props.get("card.profile.device").cloned(),
                mute: p.get("mute").is_some_and(truthy),
                vol: cubic_to_percent(&volumes),
            });
        } else if kind == "PipeWire:Interface:Metadata" {
            for m in array(o, "metadata") {
                let key = str_of(m, "key");
                let which = match key {
                    "default.audio.sink" => "sink",
                    "default.audio.source" => "source",
                    _ => continue,
                };
                let mut value = m.get("value").cloned().unwrap_or(Value::Null);
                if let Value::String(s) = &value {
                    value = serde_json::from_str(s).unwrap_or(Value::Null);
                }
                default.insert(which, value.get("name").and_then(Value::as_str).map(String::from));
            }
        }
    }

    // Uma entrada por saída/entrada; numa placa com várias portas disponíveis (alto-falantes, fones),
    // uma por porta
    let mut state = State::default();
    for mut n in nodes.clone() {
        let dev = n.device.and_then(|d| devices.get(&d));
        route_volume(&mut n, dev);
        let direction = n.kind.direction();
        let routes: Vec<&Value> = dev
            .map(|d| {
                d.routes
                    .iter()
                    .filter(|r| {
                        let profiles = array(r, "profiles");
                        str_of(r, "direction") == direction
                            && str_of(r, "available") != "no"
                            // só as portas do perfil ativo (no A2DP, o Handsfree do fone não vale)
                            && (r.get("profiles").is_none() || profiles.iter().any(|p| Some(p) == d.profile.as_ref()))
                            && n.profile_dev.as_ref().is_some_and(|pd| array(r, "devices").contains(pd))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let active = dev.and_then(|d| {
            d.active
                .iter()
                .find(|r| str_of(r, "direction") == direction && r.get("device") == n.profile_dev.as_ref())
                .and_then(|r| r.get("index").cloned())
        });
        let is_default = default.get(n.kind.key()).cloned().flatten().as_deref() == Some(n.name.as_str());
        let dev_desc = dev.map(|d| d.desc.clone()).unwrap_or_default();
        let list = if n.kind == Kind::Sink { &mut state.sinks } else { &mut state.sources };
        if routes.len() > 1 {
            for r in routes {
                list.push(Entry {
                    node: n.clone(),
                    port: r.get("name").and_then(Value::as_str).map(String::from),
                    title: str_of(r, "description").to_string(),
                    sub: dev_desc.clone(),
                    active: is_default && r.get("index") == active.as_ref(),
                });
            }
        } else {
            let r = routes.first();
            let title = r.map(|r| str_of(r, "description")).filter(|t| !t.is_empty()).map(String::from).unwrap_or_else(|| n.desc.clone());
            let sub = if r.is_some() { dev_desc } else { String::new() };
            list.push(Entry { node: n.clone(), port: None, sub: if sub != title { sub } else { String::new() }, title, active: is_default });
        }
    }
    // O "atual" é o node com o volume da porta ativa já aplicado
    let applied: Vec<Node> = nodes
        .into_iter()
        .map(|mut n| {
            let dev = n.device.and_then(|d| devices.get(&d));
            route_volume(&mut n, dev);
            n
        })
        .collect();
    let current = |kind: Kind| {
        let name = default.get(kind.key()).cloned().flatten()?;
        applied.iter().find(|n| n.kind == kind && n.name == name).cloned()
    };
    state.current_sink = current(Kind::Sink);
    state.current_source = current(Kind::Source);
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Uma placa com a saída de linha ativa, como a da VM do Debian 13 (o `dump` do test_audio.py).
    fn dump(node_volume: f64, route_props: Value) -> Vec<Value> {
        let route = json!({"index": 0, "direction": "Output", "device": 1, "name": "analog-output-lineout",
                           "description": "Line Out", "profiles": [1], "devices": [1], "available": "yes"});
        let mut active = route.clone();
        active["props"] = route_props;
        vec![
            json!({"id": 40, "type": "PipeWire:Interface:Device",
                   "info": {"props": {"device.description": "Built-in Audio"},
                            "params": {"EnumRoute": [route], "Route": [active], "Profile": [{"index": 1}]}}}),
            json!({"id": 50, "type": "PipeWire:Interface:Node",
                   "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.pci", "device.id": 40,
                                      "card.profile.device": 1, "node.description": "Built-in Audio"},
                            "params": {"Props": [{"channelVolumes": [node_volume, node_volume], "mute": false}]}}}),
            json!({"id": 60, "type": "PipeWire:Interface:Metadata",
                   "metadata": [{"key": "default.audio.sink", "value": {"name": "alsa_output.pci"}}]}),
        ]
    }

    #[test]
    fn card_volume_comes_from_the_active_route() {
        // wpctl set-volume 0.90 mudou o Route (0.9³) e deixou o Props do node em 0.4³
        let state = read_state(&dump(0.4f64.powi(3), json!({"channelVolumes": [0.9f64.powi(3), 0.9f64.powi(3)], "mute": true})));
        let sink = state.current_sink.unwrap();
        assert_eq!((sink.vol, sink.mute), (90, true));
        assert_eq!(state.sinks.len(), 1);
        assert!(state.sinks[0].active);
        assert_eq!((state.sinks[0].title.as_str(), state.sinks[0].sub.as_str()), ("Line Out", "Built-in Audio"));
    }

    #[test]
    fn without_route_volume_the_node_props_count() {
        assert_eq!(read_state(&dump(0.4f64.powi(3), json!({}))).current_sink.unwrap().vol, 40);
    }

    #[test]
    fn two_ports_give_two_entries_and_only_the_active_one_is_marked() {
        let mut objects = dump(0.5, json!({}));
        let hp = json!({"index": 1, "direction": "Output", "device": 1, "name": "analog-output-headphones",
                        "description": "Headphones", "profiles": [1], "devices": [1], "available": "yes"});
        let off = json!({"index": 2, "direction": "Output", "device": 1, "name": "hdmi", "description": "HDMI",
                         "profiles": [1], "devices": [1], "available": "no"});
        objects[0]["info"]["params"]["EnumRoute"].as_array_mut().unwrap().extend([hp, off]);
        let state = read_state(&objects);
        let names: Vec<(&str, bool)> = state.sinks.iter().map(|e| (e.port.as_deref().unwrap(), e.active)).collect();
        assert_eq!(names, [("analog-output-lineout", true), ("analog-output-headphones", false)]);
    }

    #[test]
    fn dump_with_updates_during_the_read() {
        let text = r#"[{"id": 1, "type": "PipeWire:Interface:Node", "info": {}},
                       {"id": 2, "type": "PipeWire:Interface:Metadata", "metadata": [{"key": "a", "value": 1}, {"key": "b", "value": 2}]}]
                      [{"id": 1},
                       {"id": 2, "type": "PipeWire:Interface:Metadata", "metadata": [{"key": "b", "value": 3}]},
                       {"id": 3, "type": "PipeWire:Interface:Device"}]"#;
        let objects = parse_dump(text).unwrap();
        let ids: Vec<i64> = objects.iter().map(|o| o["id"].as_i64().unwrap()).collect();
        assert_eq!(ids, [2, 3]);
        assert_eq!(objects[0]["metadata"], json!([{"key": "a", "value": 1}, {"key": "b", "value": 3}]));
        assert!(parse_dump("[{").is_err());
        assert!(parse_dump("").unwrap().is_empty());
    }

    #[test]
    fn default_given_as_a_json_string_and_python_rounding() {
        let mut objects = dump(0.5, json!({}));
        objects[2]["metadata"][0]["value"] = json!("{\"name\": \"alsa_output.pci\"}");
        assert!(read_state(&objects).current_sink.is_some());
        assert_eq!(cubic_to_percent(&[]), 0);
        assert_eq!(cubic_to_percent(&[0.125, 0.001]), 50);
        assert_eq!(cubic_to_percent(&[0.9f64.powi(3)]), 90);
    }
}
