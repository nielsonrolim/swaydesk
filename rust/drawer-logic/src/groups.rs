//! Os grupos da grade, criados por quem usa: o porte da parte de grupos de `sway_drawer.py` (#59).
//!
//! Um app entra num grupo pela lista (`apps`) ou por uma das categorias (as chaves de
//! [`crate::CATEGORIES`]), e `hidden` tira o que entrou pela categoria. `default` é a chave de um grupo
//! padrão ainda não renomeado: o nome mostrado é a tradução dela, na língua da sessão. Renomear apaga
//! a chave, e o nome passa a ser só o que foi digitado.
//!
//! O arquivo é o mesmo do Python (`~/.local/share/sway-drawer/groups.json`), no mesmo formato: os
//! dois lados leem e gravam o que o outro gravou.
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::{categories_of, normalize, App};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Group {
    pub name: String,
    pub categories: Vec<String>,
    pub apps: Vec<String>,
    pub hidden: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub default: String,
}

impl Group {
    pub fn named(name: &str) -> Group {
        Group { name: name.to_string(), ..Group::default() }
    }
}

/// Os três grupos de quem ainda não tem o arquivo, como no COSMIC: (chave, categorias).
pub const DEFAULT_GROUPS: &[(&str, &[&str])] =
    &[("Office", &["Office"]), ("System", &["System", "Settings"]), ("Utilities", &["Utility"])];

fn default_categories(key: &str) -> Option<&'static [&'static str]> {
    DEFAULT_GROUPS.iter().find(|(k, _)| *k == key).map(|(_, categories)| *categories)
}

pub fn default_groups() -> Vec<Group> {
    DEFAULT_GROUPS
        .iter()
        .map(|(key, categories)| Group {
            name: key.to_string(),
            categories: categories.iter().map(|c| c.to_string()).collect(),
            default: key.to_string(),
            ..Group::default()
        })
        .collect()
}

pub fn display_name(group: &Group, translate: &dyn Fn(&str) -> String) -> String {
    if group.default.is_empty() {
        group.name.clone()
    } else {
        translate(&group.default)
    }
}

pub fn in_group(app: &App, group: &Group) -> bool {
    if group.hidden.iter().any(|id| id == app.id()) {
        return false;
    }
    if group.apps.iter().any(|id| id == app.id()) {
        return true;
    }
    let categories = categories_of(app);
    group.categories.iter().any(|c| categories.contains(c.as_str()))
}

/// Os apps do grupo, na ordem em que vieram (a grade já os passa por nome).
pub fn apps_in_group<'a>(apps: &'a [App], group: &Group) -> Vec<&'a App> {
    apps.iter().filter(|app| in_group(app, group)).collect()
}

/// O `str.strip()` do Python: além do espaço do Unicode, tira os separadores \x1c-\x1f. Assim um nome
/// é o mesmo nas duas versões.
pub fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// Uma lista de textos, sem repetidos e na ordem (o `dict.fromkeys` do Python).
fn strings(value: Option<&Value>, what: &str) -> io::Result<Vec<String>> {
    let Some(value) = value else { return Ok(Vec::new()) };
    let list = value.as_array().ok_or_else(|| bad(&format!("{what} is not a list of strings")))?;
    let mut out: Vec<String> = Vec::new();
    for item in list {
        let text = item.as_str().ok_or_else(|| bad(&format!("{what} is not a list of strings")))?;
        if !out.iter().any(|seen| seen == text) {
            out.push(text.to_string());
        }
    }
    Ok(out)
}

/// Os grupos salvos. Sem o arquivo, os padrões. Um arquivo ilegível ou malformado é erro
/// (`InvalidData`, ou o erro de leitura): tratar como vazio apagaria os grupos ao salvar.
pub fn load_groups(path: &Path) -> io::Result<Vec<Group>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(default_groups()),
        Err(err) => return Err(err),
    };
    let data: Value = serde_json::from_str(&text).map_err(|err| bad(&err.to_string()))?;
    let entries = data.as_array().ok_or_else(|| bad("the groups file is not a list"))?;
    let mut groups = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for entry in entries {
        let object = entry.as_object().ok_or_else(|| bad("a group has no name"))?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .map(py_strip)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| bad("a group has no name"))?
            .to_string();
        let categories = strings(object.get("categories"), "categories")?;
        let mut default = match object.get("default") {
            None => {
                // Gravado antes da chave existir, com o nome e as categorias do padrão
                let same = default_categories(&name).is_some_and(|wanted| {
                    !categories.is_empty()
                        && wanted.len() == categories.len()
                        && wanted.iter().all(|c| categories.iter().any(|have| have == c))
                });
                if same { name.clone() } else { String::new() }
            }
            Some(Value::String(key)) => key.clone(),
            Some(_) => String::new(), // não é texto: vale como nome comum
        };
        // Uma chave desconhecida, ou repetida, vale como nome comum
        if !default.is_empty() && (default_categories(&default).is_none() || seen.contains(&default)) {
            default.clear();
        }
        if !default.is_empty() {
            seen.push(default.clone());
        }
        groups.push(Group {
            name,
            categories,
            apps: strings(object.get("apps"), "apps")?,
            hidden: strings(object.get("hidden"), "hidden")?,
            default,
        });
    }
    Ok(groups)
}

/// Grava como o Python (`json.dumps(..., ensure_ascii=False, indent=2)` e uma quebra de linha).
pub fn save_groups(path: &Path, groups: &[Group]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(groups).map_err(|err| bad(&err.to_string()))?;
    text.push('\n');
    swaydesk_common::text::atomic_write(path, text.as_bytes(), 0o600)
}

/// `name`, ou "name 2", "name 3"... se já houver um grupo que apareça com ele. `skip`: o índice do
/// grupo que está sendo renomeado, que pode manter o próprio nome.
pub fn unique_name(groups: &[Group], name: &str, skip: Option<usize>, translate: &dyn Fn(&str) -> String) -> String {
    let taken: Vec<String> = groups
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != skip)
        .map(|(_, g)| normalize(&display_name(g, translate)))
        .collect();
    let mut candidate = name.to_string();
    let mut n = 2;
    while taken.contains(&normalize(&candidate)) {
        candidate = format!("{name} {n}");
        n += 1;
    }
    candidate
}

pub fn add_group(groups: &[Group], name: &str, translate: &dyn Fn(&str) -> String) -> Vec<Group> {
    let mut out = groups.to_vec();
    out.push(Group::named(&unique_name(groups, py_strip(name), None, translate)));
    out
}

/// O nome digitado substitui a chave do grupo padrão: dali em diante ele não é mais traduzido.
pub fn rename_group(groups: &[Group], index: usize, name: &str, translate: &dyn Fn(&str) -> String) -> Vec<Group> {
    let new = unique_name(groups, py_strip(name), Some(index), translate);
    groups
        .iter()
        .enumerate()
        .map(|(i, g)| if i == index { Group { name: new.clone(), default: String::new(), ..g.clone() } } else { g.clone() })
        .collect()
}

pub fn delete_group(groups: &[Group], index: usize) -> Vec<Group> {
    groups.iter().enumerate().filter(|(i, _)| *i != index).map(|(_, g)| g.clone()).collect()
}

pub fn add_app(groups: &[Group], index: usize, app_id: &str) -> Vec<Group> {
    groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            if i != index {
                return g.clone();
            }
            let mut g = g.clone();
            if !g.apps.iter().any(|id| id == app_id) {
                g.apps.push(app_id.to_string());
            }
            g.hidden.retain(|id| id != app_id);
            g
        })
        .collect()
}

/// Tira o app da lista e, se ele ainda entrar pela categoria, o esconde.
pub fn remove_app(groups: &[Group], index: usize, app: &App) -> Vec<Group> {
    let categories = categories_of(app);
    groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            if i != index {
                return g.clone();
            }
            let mut g = g.clone();
            if g.categories.iter().any(|c| categories.contains(c.as_str())) && !g.hidden.iter().any(|id| id == app.id()) {
                g.hidden.push(app.id().to_string());
            }
            g.apps.retain(|id| id != app.id());
            g
        })
        .collect()
}
