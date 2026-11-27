//! O porte de `tests/test_drawer.py` (a parte sem GTK). Os valores da calculadora e do
//! `format_number` vêm do `sway_drawer.py`, rodado com as mesmas entradas.
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use swaydesk_drawer_logic::*;

fn app(name: &str) -> AppFields {
    AppFields { id: name.to_lowercase().replace(' ', "-"), name: name.to_string(), ..Default::default() }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Uma pasta temporária só deste teste, apagada no fim.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("swaydesk-drawer-logic-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Categorias

#[test]
fn app_goes_to_every_main_category_it_has() {
    let kdenlive = App::new(AppFields { categories: strings(&["AudioVideo", "Video", "Graphics"]), ..app("Kdenlive") });
    assert_eq!(categories_of(&kdenlive), HashSet::from(["AudioVideo", "Graphics"]));
}

#[test]
fn app_without_a_known_category_is_other() {
    let custom = App::new(AppFields { categories: strings(&["X-Custom"]), ..app("Coisa") });
    assert_eq!(categories_of(&custom), HashSet::from([OTHER]));
    assert_eq!(categories_of(&App::new(app("Nada"))), HashSet::from([OTHER]));
}

#[test]
fn only_used_categories_in_fixed_order_with_other_last() {
    let apps = vec![
        App::new(AppFields { categories: strings(&["Utility"]), ..app("A") }),
        App::new(app("B")),
        App::new(AppFields { categories: strings(&["Network"]), ..app("C") }),
    ];
    assert_eq!(used_categories(&apps), vec!["Network", "Utility", OTHER]);
    assert!(used_categories(&[]).is_empty());
}

#[test]
fn parse_categories_drops_empty_parts() {
    assert_eq!(parse_categories("GTK;Utility;"), strings(&["GTK", "Utility"]));
    assert!(parse_categories("").is_empty());
}

// Busca

fn search_apps() -> Vec<App> {
    let mut apps: Vec<App> = vec![
        AppFields {
            id: "firefox".into(),
            generic: "Web Browser".into(),
            keywords: strings(&["internet", "www"]),
            ..app("Firefox Web Browser")
        },
        AppFields {
            id: "org.gnome.TextEditor".into(),
            keywords: strings(&["notepad"]),
            executable: "gnome-text-editor".into(),
            ..app("Text Editor")
        },
        AppFields { id: "settings".into(), comment: "Ajustes do sistema".into(), ..app("Configurações") },
        AppFields { id: "thunar".into(), executable: "/usr/bin/thunar".into(), ..app("Files") },
        AppFields { id: "fire".into(), ..app("Fire Starter") },
    ]
    .into_iter()
    .map(App::new)
    .collect();
    sort_apps(&mut apps);
    apps
}

fn ids(query: &str) -> Vec<String> {
    let apps = search_apps();
    search(&apps, query).iter().map(|a| a.id().to_string()).collect()
}

#[test]
fn empty_query_keeps_everything() {
    assert_eq!(ids("  ").len(), 5);
    assert_eq!(score(&App::new(app("X")), ""), Some(0));
}

#[test]
fn name_prefix_comes_before_a_word_inside() {
    assert_eq!(ids("fire"), ["fire", "firefox"]);
}

#[test]
fn case_and_accents_do_not_matter() {
    assert_eq!(ids("CONFIGURACOES"), ["settings"]);
    assert_eq!(ids("configurações"), ["settings"]);
}

#[test]
fn keywords_executable_and_comment_also_match() {
    assert_eq!(ids("notepad"), ["org.gnome.TextEditor"]);
    assert_eq!(ids("thun"), ["thunar"]);
    assert_eq!(ids("ajustes"), ["settings"]);
}

#[test]
fn every_word_must_match_something() {
    assert_eq!(ids("editor notepad"), ["org.gnome.TextEditor"]);
    assert!(ids("editor zzz").is_empty());
}

#[test]
fn name_match_ranks_above_comment_match() {
    let apps: Vec<App> = vec![
        App::new(AppFields { comment: "um editor de texto".into(), ..app("Zed") }),
        App::new(app("Editor")),
    ];
    let found: Vec<&str> = search(&apps, "editor").iter().map(|a| a.id()).collect();
    assert_eq!(found, ["editor", "zed"]);
}

#[test]
fn sort_is_by_normalized_name_then_id() {
    let mut apps: Vec<App> = vec![app("beta"), app("Álfa"), AppFields { id: "a2".into(), ..app("alfa") }]
        .into_iter()
        .map(App::new)
        .collect();
    sort_apps(&mut apps);
    let order: Vec<&str> = apps.iter().map(|a| a.id()).collect();
    assert_eq!(order, ["a2", "álfa", "beta"]);
}

// Calculadora

#[test]
fn operations() {
    assert_eq!(calculate("2+3*4"), Some(14.0));
    assert_eq!(calculate("(2+3)*4"), Some(20.0));
    assert_eq!(calculate("2^10"), Some(1024.0));
    assert_eq!(calculate("7 // 2"), Some(3.0));
    assert_eq!(calculate("1/3"), Some(1.0 / 3.0));
    assert_eq!(calculate("-2 * 3"), Some(-6.0));
}

#[test]
fn comma_is_a_decimal_separator() {
    assert_eq!(calculate("2,5*2"), Some(5.0));
    assert_eq!(calculate("1,5+1,5"), Some(3.0));
    assert_eq!(calculate("1.000,5+1"), None);
}

#[test]
fn not_a_calculation() {
    for text in ["", "firefox", "42", "-5", "2+", "1/0", "9^99999", "__import__('os')", "2 + x", "(2)", "-(3)", "2(3)",
                 "1+1)", "(1+1", "2***3", "1.2.3+1", "1e5+1", "3 % 0", "3 // 0", "0**-1", "(-8)**0.5", "01+1"] {
        assert_eq!(calculate(text), None, "{text:?}");
    }
}

#[test]
fn qalc_candidates() {
    for text in ["5 km to mi", "5 KM TO MI", "100 °F -> °C", "5km→mi", "-5 °C to °F", "(2 m + 3 m) to cm",
                 "1 GiB to MB", "10 USD to BRL", "2 h + 30 min", "3m*2", "sqrt(2)", "SQRT( 2 )", "log(100)"] {
        assert!(qalc_candidate(text), "{text:?}");
    }
}

#[test]
fn app_names_never_reach_qalc() {
    let long = format!("1 m to {}", "x".repeat(200));
    for text in ["", "firefox", "xfce4-terminal", "0 A.D.", "7zip", "2048", "python3.12", "gimp-2.10", "mp3 to wav",
                 "Back to the Future 2", "Steam (Runtime 3)", "Area 51", "3d printer", "2fa app", "10 USD in BRL",
                 "2+2", "42", "5 km to mi\nrm", long.as_str()] {
        assert!(!qalc_candidate(text), "{text:?}");
    }
}

#[test]
fn side_effects_never_reach_qalc() {
    // O command() do qalc roda um programa; load, export, save e plot mexem em arquivos ou no gnuplot
    for text in ["1 to command(\"x\")", "1 -> COMMAND(\"x", "1 m to load(x)", "2 m * export(a)", "1 to save(1, x)",
                 "sqrt(2) + plot(x)", "1 to команда(x)", "1 to kommando(x)"] {
        assert!(!qalc_candidate(text), "{text:?}");
    }
}

/// Os fins de linhas de itens de 112 com 6 entre elas, a partir de `top`, `per_row` itens por linha
fn rows(top: f64, count: usize, per_row: usize) -> Vec<f64> {
    (0..count).map(|i| top + (i / per_row) as f64 * 118.0 + 112.0).collect()
}

#[test]
fn whole_rows_only() {
    // 400 de área e 20 itens em 4 linhas (466 de conteúdo): três linhas inteiras (até 348), e a sobra embaixo
    let bottoms = rows(0.0, 20, 6);
    assert_eq!(row_gap(400.0, 466.0, &bottoms), 400.0 - 348.0);
    // A sobra já contada na área dá o mesmo resultado (sem vaivém)
    assert_eq!(row_gap(400.0, 466.0, &bottoms), 52.0);
    // Exatas três linhas: nenhuma sobra
    assert_eq!(row_gap(348.0, 466.0, &bottoms), 0.0);
}

#[test]
fn whole_rows_with_the_favorites_on_top() {
    // Uma linha de favoritos, 12 de espaço e depois a grade: a medida segue os itens, sem passo fixo
    let mut bottoms = rows(0.0, 3, 6);
    bottoms.extend(rows(112.0 + 12.0, 20, 6));
    assert_eq!(row_gap(400.0, 590.0, &bottoms), 400.0 - (124.0 + 118.0 + 112.0));
    // Uma linha mais alta (nomes de três linhas) também
    assert_eq!(row_gap(300.0, 500.0, &[112.0, 112.0, 260.0, 380.0]), 300.0 - 260.0);
}

#[test]
fn no_gap_when_everything_fits_or_is_unknown() {
    let bottoms = rows(0.0, 20, 6);
    assert_eq!(row_gap(600.0, 466.0, &bottoms), 0.0);
    assert_eq!(row_gap(400.0, 466.0, &[]), 0.0); // antes do primeiro desenho
    assert_eq!(row_gap(400.0, 466.0, &[0.0, 0.0]), 0.0); // itens ainda sem tamanho
    assert_eq!(row_gap(0.0, 466.0, &bottoms), 0.0);
    assert_eq!(row_gap(80.0, 466.0, &bottoms), 0.0); // nem uma linha cabe
}

#[test]
fn qalc_output() {
    assert_eq!(qalc_result("3 mi + 188 yd\n", true).as_deref(), Some("3 mi + 188 yd"));
    assert_eq!(qalc_result("  1.414213562  \nextra\n", true).as_deref(), Some("1.414213562"));
    assert_eq!(qalc_result("0 B\n", false), None); // o qalc falhou
    assert_eq!(qalc_result("", true), None);
}

#[test]
fn same_results_as_python() {
    let cases: &[(&str, f64)] = &[
        ("-2**2", -4.0),
        ("2**-1", 0.5),
        ("-7 % 3", 2.0),
        ("7 % -3", -2.0),
        ("-7 // 2", -4.0),
        ("00+1", 1.0),
        ("01.5+1", 2.5),
        ("2**3**2", 512.0),
        ("5.+.5", 5.5),
        ("7.5//2", 3.0),
        ("-7.5%2", 0.5),
        ("10**15+0.5", 1000000000000000.5),
        ("(1+2)*(3-4)/5", -0.6),
        ("+-+3*2", -6.0),
        ("10/4", 2.5),
        ("2**0.5", std::f64::consts::SQRT_2),
        ("0.1+0.2", 0.30000000000000004),
        ("2 \t+ 3", 5.0),
    ];
    for (text, expected) in cases {
        assert_eq!(calculate(text), Some(*expected), "{text:?}");
    }
}

#[test]
fn huge_or_deep_input_is_not_a_calculation_and_does_not_panic() {
    let big = "9".repeat(90);
    assert_eq!(calculate(&["1"; 5000].join("+")), None); // longa demais
    assert_eq!(calculate(&format!("{}1+1{}", "(".repeat(150), ")".repeat(150))), None);
    assert_eq!(calculate(&format!("{big}-{big}")), Some(0.0));
    assert_eq!(calculate(&"9".repeat(MAX_CALC_LENGTH + 1)), None);
}

#[test]
fn format() {
    assert_eq!(format_number(14.0, "."), "14");
    assert_eq!(format_number(2.5, ","), "2,5");
    assert_eq!(format_number(1.0 / 3.0, "."), "0.3333333333");
    assert_eq!(format_number(-0.0, "."), "0");
    let cases: &[(f64, &str)] = &[
        (1e17, "1e+17"),
        (1.5e-5, "1.5e-05"),
        (123456789012.5, "1.23456789e+11"),
        (1e15, "1e+15"),
        (1000000000000000.5, "1e+15"),
        (0.0001234, "0.0001234"),
        (0.00001234, "1.234e-05"),
        (-2.5e20, "-2.5e+20"),
        (9999999999.5, "1e+10"),
        (1e-10, "1e-10"),
        (1.152921504606847e18, "1.152921505e+18"),
        (std::f64::consts::SQRT_2, "1.414213562"),
        (0.30000000000000004, "0.3"),
    ];
    for (value, expected) in cases {
        assert_eq!(format_number(*value, "."), *expected, "{value:?}");
    }
    assert_eq!(format_number(1.5e-5, ","), "1,5e-05");
}

// Terminal

#[test]
fn field_codes_leave_and_terminal_comes_first() {
    assert_eq!(terminal_command("htop --tree %U", "kitty"), Some(strings(&["kitty", "-e", "htop", "--tree"])));
    assert_eq!(terminal_command(r#"sh -c "echo 100%%""#, "kitty"), Some(strings(&["kitty", "-e", "sh", "-c", "echo 100%"])));
}

#[test]
fn bad_or_empty_command() {
    assert_eq!(terminal_command(r#"vim "unclosed"#, "kitty"), None);
    assert_eq!(terminal_command("%F", "kitty"), None);
    assert_eq!(terminal_command("", "kitty"), None);
}

#[test]
fn the_terminal_is_the_sway_terminal_helper_then_kitty() {
    // O sway-terminal (o terminal da config) ao lado; sem ele, o kitty de antes, se instalado
    let helper = PathBuf::from("/x/sway-terminal");
    assert_eq!(terminal_program(Some(&helper), true).as_deref(), Some("/x/sway-terminal"));
    assert_eq!(terminal_program(None, true).as_deref(), Some("kitty"));
    assert_eq!(terminal_program(None, false), None);
}

// Fixados

#[test]
fn without_a_file_the_nwg_drawer_pins_come_over() {
    let dir = TempDir::new("pins-legacy");
    let legacy = dir.path().join("nwg-pin-cache");
    fs::write(&legacy, "firefox.desktop\nkitty\n\nfirefox\n").unwrap();
    assert_eq!(load_pins(&dir.path().join("pinned"), Some(&legacy)).unwrap(), strings(&["firefox", "kitty"]));
}

#[test]
fn own_file_wins_and_saving_round_trips() {
    let dir = TempDir::new("pins-own");
    let legacy = dir.path().join("nwg-pin-cache");
    fs::write(&legacy, "old\n").unwrap();
    let path = dir.path().join("sub").join("pinned");
    save_pins(&path, &strings(&["a", "b"])).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "a\nb\n");
    assert_eq!(load_pins(&path, Some(&legacy)).unwrap(), strings(&["a", "b"]));
}

#[test]
fn no_file_at_all() {
    let dir = TempDir::new("pins-none");
    assert!(load_pins(&dir.path().join("pinned"), Some(&dir.path().join("none"))).unwrap().is_empty());
    assert!(load_pins(&dir.path().join("pinned"), None).unwrap().is_empty());
}

#[test]
fn unreadable_file_is_an_error_instead_of_looking_empty() {
    let dir = TempDir::new("pins-bad");
    let folder = dir.path().join("pinned");
    fs::create_dir(&folder).unwrap();
    assert!(load_pins(&folder, None).is_err());
    let binary = dir.path().join("binary");
    fs::write(&binary, b"\xff\xfe\n").unwrap();
    assert!(load_pins(&binary, None).is_err());
}

#[test]
fn toggle() {
    assert_eq!(toggle_pin(&strings(&["a"]), "b"), strings(&["a", "b"]));
    assert_eq!(toggle_pin(&strings(&["a", "b"]), "a"), strings(&["b"]));
}

// Arquivos

fn file_tree(name: &str) -> TempDir {
    let dir = TempDir::new(name);
    for rel in ["Docs/Relatório anual.pdf", "Docs/deep/a/b/c/d/e/relatorio-fundo.txt", "Docs/.oculto/relatorio.txt",
                "Docs/notas.txt", "Fotos/relatorios/x.png"] {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }
    dir
}

fn names(found: &[(PathBuf, bool)]) -> Vec<String> {
    found.iter().map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

#[test]
fn finds_by_name_without_accents_shallow_first_and_skips_hidden() {
    let dir = file_tree("files-find");
    let roots = [dir.path().join("Docs"), dir.path().join("Fotos")];
    let found = find_files(&roots, "relatorio", &|| false, FileLimits::default());
    let names = names(&found);
    assert_eq!(names[..2], ["Relatório anual.pdf", "relatorios"]);
    assert!(found.iter().any(|(path, is_dir)| *path == dir.path().join("Fotos/relatorios") && *is_dir));
    assert!(!names.contains(&"relatorio.txt".to_string()));
    assert!(!names.contains(&"relatorio-fundo.txt".to_string())); // mais fundo que max_depth
}

#[test]
fn short_query_limit_and_cancel() {
    let dir = file_tree("files-limits");
    let roots = [dir.path().to_path_buf()];
    assert!(find_files(&roots, "re", &|| false, FileLimits::default()).is_empty());
    assert_eq!(find_files(&roots, "rel", &|| false, FileLimits { limit: 1, ..FileLimits::default() }).len(), 1);
    assert!(find_files(&roots, "rel", &|| true, FileLimits::default()).is_empty());
    assert!(find_files(&roots, "rel", &|| false, FileLimits { max_visited: 1, ..FileLimits::default() }).is_empty());
}

#[test]
fn folder_links_are_not_followed() {
    let dir = file_tree("files-links");
    std::os::unix::fs::symlink(dir.path().join("Fotos"), dir.path().join("Docs/link")).unwrap();
    let found = find_files(&[dir.path().join("Docs")], "relatorios", &|| false, FileLimits::default());
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn roots_leave_out_home_missing_and_nested_folders() {
    let dir = TempDir::new("roots");
    let home = dir.path();
    for rel in ["Docs/Sub", "Pics"] {
        fs::create_dir_all(home.join(rel)).unwrap();
    }
    let real = |rel: &str| fs::canonicalize(home.join(rel)).unwrap();
    let special = [
        Some(home.to_path_buf()),
        Some(home.join("Docs")),
        None,
        Some(home.join("Missing")),
        Some(home.join("Docs/Sub")),
        Some(home.join("Pics")),
        Some(home.join("Pics")),
    ];
    assert_eq!(search_roots(home, &special), vec![real("Docs"), real("Pics")]);
}

// App

#[test]
fn app_keeps_its_fields() {
    let fields = AppFields { keywords: strings(&["Ação"]), ..app("Configurações") };
    let app = App::new(fields.clone());
    assert_eq!(app.fields, fields);
    assert_eq!(app.id(), "configurações");
    assert_eq!(search(&[app], "acao").len(), 1);
}
