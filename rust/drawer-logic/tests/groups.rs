//! O porte de `GroupTests` de `tests/test_drawer.py`.
use std::fs;
use std::path::PathBuf;

use swaydesk_drawer_logic::groups::*;
use swaydesk_drawer_logic::{App, AppFields};

fn app(name: &str, categories: &[&str]) -> App {
    App::new(AppFields {
        id: name.to_lowercase(),
        name: name.to_string(),
        categories: categories.iter().map(|c| c.to_string()).collect(),
        ..Default::default()
    })
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn same(text: &str) -> String {
    text.to_string()
}

fn upper(text: &str) -> String {
    text.to_uppercase()
}

fn pt(text: &str) -> String {
    match text {
        "Office" => "Escritório",
        "System" => "Sistema",
        "Utilities" => "Utilitários",
        other => other,
    }
    .to_string()
}

struct Fixture {
    dir: PathBuf,
    writer: App,
    term: App,
    game: App,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("swaydesk-groups-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Fixture {
            dir,
            writer: app("Writer", &["Office"]),
            term: app("Terminal", &["System"]),
            game: app("Jogo", &["Game"]),
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("sub").join("groups.json")
    }

    fn write(&self, text: &str) -> PathBuf {
        let path = self.path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn apps(&self) -> Vec<App> {
        vec![self.game.clone(), self.term.clone(), self.writer.clone()]
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn ids(apps: &[&App]) -> Vec<String> {
    apps.iter().map(|a| a.id().to_string()).collect()
}

#[test]
fn an_app_belongs_by_category_or_by_list() {
    let f = Fixture::new("belongs");
    let group = Group { name: "Trabalho".into(), categories: strings(&["Office"]), apps: strings(&["jogo"]), ..Group::default() };
    assert_eq!(ids(&apps_in_group(&f.apps(), &group)), ["jogo", "writer"]);
}

#[test]
fn hidden_wins_over_the_category() {
    let f = Fixture::new("hidden");
    let group = Group { name: "Trabalho".into(), categories: strings(&["Office"]), hidden: strings(&["writer"]), ..Group::default() };
    assert!(apps_in_group(&f.apps(), &group).is_empty());
}

#[test]
fn defaults_without_a_file_show_in_the_session_language() {
    let f = Fixture::new("defaults");
    let groups = load_groups(&f.path()).unwrap();
    let names: Vec<String> = groups.iter().map(|g| display_name(g, &upper)).collect();
    assert_eq!(names, ["OFFICE", "SYSTEM", "UTILITIES"]);
    assert_eq!(ids(&apps_in_group(&f.apps(), &groups[1])), ["terminal"]);
    assert!(!f.path().exists()); // só grava quando algo muda
}

#[test]
fn saving_round_trips() {
    let f = Fixture::new("roundtrip");
    let groups = add_app(&add_group(&default_groups(), "Ação", &same), 3, "kitty");
    save_groups(&f.path(), &groups).unwrap();
    assert_eq!(load_groups(&f.path()).unwrap(), groups);
}

#[test]
fn the_file_is_written_like_python() {
    // O mesmo texto do json.dumps(..., ensure_ascii=False, indent=2) do sway_drawer.py
    let f = Fixture::new("format");
    let groups = vec![
        Group { name: "Office".into(), categories: strings(&["Office"]), default: "Office".into(), ..Group::default() },
        Group { name: "Ação".into(), apps: strings(&["kitty"]), ..Group::default() },
    ];
    save_groups(&f.path(), &groups).unwrap();
    let expected = "[\n  {\n    \"name\": \"Office\",\n    \"categories\": [\n      \"Office\"\n    ],\n    \"apps\": [],\n    \
                    \"hidden\": [],\n    \"default\": \"Office\"\n  },\n  {\n    \"name\": \"Ação\",\n    \"categories\": [],\n    \
                    \"apps\": [\n      \"kitty\"\n    ],\n    \"hidden\": []\n  }\n]\n";
    assert_eq!(fs::read_to_string(f.path()).unwrap(), expected);
}

#[test]
fn default_names_follow_the_session_language_until_renamed() {
    let f = Fixture::new("language");
    // Um grupo do usuário chamado "Office" numa sessão em português não vira "Escritório"
    let groups = add_group(&load_groups(&f.path()).unwrap(), "Office", &pt);
    let groups = rename_group(&groups, 1, "Ferramentas", &pt);
    save_groups(&f.path(), &groups).unwrap();
    let loaded = load_groups(&f.path()).unwrap();
    let shown: Vec<String> = loaded.iter().map(|g| display_name(g, &pt)).collect();
    assert_eq!(shown, ["Escritório", "Ferramentas", "Utilitários", "Office"]);
    let english: Vec<String> = loaded.iter().map(|g| display_name(g, &same)).collect();
    assert_eq!(english, ["Office", "Ferramentas", "Utilities", "Office"]);
}

#[test]
fn names_are_unique_by_what_is_shown() {
    let translate = |n: &str| if n == "Office" { "Escritório".to_string() } else { n.to_string() };
    let groups = add_group(&default_groups(), "escritorio", &translate);
    assert_eq!(groups[3].name, "escritorio 2");
}

#[test]
fn files_from_before_the_key_keep_the_english_defaults_translated() {
    let f = Fixture::new("before-key");
    let path = f.write(r#"[{"name": "Office", "categories": ["Office"]}, {"name": "System", "categories": []}]"#);
    let defaults: Vec<String> = load_groups(&path).unwrap().into_iter().map(|g| g.default).collect();
    assert_eq!(defaults, ["Office", ""]); // o segundo não tem as categorias do padrão
}

#[test]
fn a_repeated_default_key_counts_only_once() {
    let f = Fixture::new("repeated");
    let path = f.write(r#"[{"name": "Office", "default": "Office"}, {"name": "Office", "categories": ["Office"]}]"#);
    let defaults: Vec<String> = load_groups(&path).unwrap().into_iter().map(|g| g.default).collect();
    assert_eq!(defaults, ["Office", ""]);
}

#[test]
fn unknown_default_key_is_a_plain_name() {
    let f = Fixture::new("unknown");
    let path = f.write(r#"[{"name": "X", "default": "Games"}, {"name": "Y", "default": 3}]"#);
    let defaults: Vec<String> = load_groups(&path).unwrap().into_iter().map(|g| g.default).collect();
    assert_eq!(defaults, ["", ""]);
}

#[test]
fn bad_file_is_an_error_instead_of_looking_empty() {
    let f = Fixture::new("bad");
    for text in ["{", r#"{"name": "x"}"#, r#"[{"apps": []}]"#, r#"[{"name": " "}]"#, r#"[{"name": "x", "apps": [1]}]"#] {
        let path = f.write(text);
        let err = load_groups(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData, "{text}");
    }
    fs::remove_file(f.path()).unwrap();
    fs::create_dir(f.path()).unwrap();
    assert!(load_groups(&f.path()).is_err());
}

#[test]
fn names_are_unique_ignoring_case_and_accents() {
    let groups = add_group(&[Group::named("Ação")], "acao", &same);
    assert_eq!(groups[1].name, "acao 2");
    assert_eq!(add_group(&groups, "Ação", &same)[2].name, "Ação 3");
}

#[test]
fn rename_keeps_its_own_name_and_avoids_the_others() {
    let groups = vec![Group::named("A"), Group::named("B")];
    assert_eq!(rename_group(&groups, 0, "a", &same)[0].name, "a");
    assert_eq!(rename_group(&groups, 0, "B", &same)[0].name, "B 2");
}

#[test]
fn names_are_stripped_like_python() {
    // O str.strip() do Python também tira os separadores \x1c-\x1f
    let groups = add_group(&[], " Jogos\x1f\n", &same);
    assert_eq!(groups[0].name, "Jogos");
}

#[test]
fn delete() {
    assert_eq!(delete_group(&[Group::named("A"), Group::named("B")], 0), vec![Group::named("B")]);
}

#[test]
fn add_then_remove_an_app_by_list() {
    let f = Fixture::new("add-remove");
    let groups = add_app(&[Group::named("A")], 0, "jogo");
    assert_eq!(groups[0].apps, ["jogo"]);
    assert_eq!(add_app(&groups, 0, "jogo"), groups);
    assert_eq!(remove_app(&groups, 0, &f.game), vec![Group::named("A")]);
}

#[test]
fn removing_an_app_that_came_by_category_hides_it_and_adding_shows_it_again() {
    let f = Fixture::new("hide-show");
    let base = Group { name: "Escritório".into(), categories: strings(&["Office"]), ..Group::default() };
    let groups = remove_app(&[base], 0, &f.writer);
    assert_eq!(groups[0].hidden, ["writer"]);
    assert!(apps_in_group(&f.apps(), &groups[0]).is_empty());
    let groups = add_app(&groups, 0, "writer");
    assert!(groups[0].hidden.is_empty());
    assert_eq!(ids(&apps_in_group(&f.apps(), &groups[0])), ["writer"]);
}

