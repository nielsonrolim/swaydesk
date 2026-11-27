//! A lógica da grade de apps, sem GTK: o porte de `home/.local/bin/sway_drawer.py`. A interface
//! (o crate `swaydesk-drawer`) usa só o que está aqui; os testes portam os de `tests/test_drawer.py`.
use std::cmp::Reverse;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

pub use swaydesk_common::text::normalize;

pub mod dock;
pub mod groups;
pub use groups::Group;

/// Os campos de um .desktop que a grade usa, como vêm do Gio.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppFields {
    /// Sem o `.desktop`
    pub id: String,
    pub name: String,
    pub generic: String,
    pub comment: String,
    pub keywords: Vec<String>,
    pub executable: String,
    /// As do .desktop, como vêm de `parse_categories`
    pub categories: Vec<String>,
    pub terminal: bool,
}

/// Um app, com os campos já normalizados para a busca (feitos uma vez, e não a cada tecla).
#[derive(Clone, Debug)]
pub struct App {
    pub fields: AppFields,
    norm: Norm,
}

#[derive(Clone, Debug, Default)]
struct Norm {
    name: String,
    generic: String,
    keywords: Vec<String>,
    executable: String,
    id: String,
    comment: String,
}

impl App {
    pub fn new(fields: AppFields) -> App {
        let executable = Path::new(&fields.executable)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let norm = Norm {
            name: normalize(&fields.name),
            generic: normalize(&fields.generic),
            keywords: fields.keywords.iter().map(|k| normalize(k)).collect(),
            executable: normalize(&executable),
            id: normalize(&fields.id),
            comment: normalize(&fields.comment),
        };
        App { fields, norm }
    }

    pub fn id(&self) -> &str {
        &self.fields.id
    }
}

/// (chave, categorias do .desktop). Um app aparece em todas as que tiver.
pub const CATEGORIES: &[(&str, &[&str])] = &[
    ("AudioVideo", &["AudioVideo", "Audio", "Video"]),
    ("Development", &["Development"]),
    ("Education", &["Education", "Science"]),
    ("Game", &["Game"]),
    ("Graphics", &["Graphics"]),
    ("Network", &["Network"]),
    ("Office", &["Office"]),
    ("Settings", &["Settings", "DesktopSettings", "HardwareSettings"]),
    ("System", &["System", "Monitor"]),
    ("Utility", &["Utility", "Accessibility", "Core"]),
];
pub const OTHER: &str = "Other";

pub fn parse_categories(text: &str) -> Vec<String> {
    text.split(';').filter(|part| !part.is_empty()).map(str::to_string).collect()
}

/// As chaves de CATEGORIES do app; sem nenhuma, só OTHER.
pub fn categories_of(app: &App) -> HashSet<&'static str> {
    let found: HashSet<&'static str> = CATEGORIES
        .iter()
        .filter(|(_key, members)| app.fields.categories.iter().any(|c| members.contains(&c.as_str())))
        .map(|(key, _members)| *key)
        .collect();
    if found.is_empty() {
        HashSet::from([OTHER])
    } else {
        found
    }
}

/// As categorias com algum app, na ordem de CATEGORIES, com OTHER no fim.
pub fn used_categories(apps: &[App]) -> Vec<&'static str> {
    let used: HashSet<&'static str> = apps.iter().flat_map(categories_of).collect();
    CATEGORIES
        .iter()
        .map(|(key, _members)| *key)
        .chain(std::iter::once(OTHER))
        .filter(|key| used.contains(key))
        .collect()
}

/// A ordem da grade: pelo nome normalizado, depois pelo id.
pub fn sort_apps(apps: &mut [App]) {
    apps.sort_by(|a, b| (&a.norm.name, &a.fields.id).cmp(&(&b.norm.name, &b.fields.id)));
}

fn term_score(app: &App, term: &str) -> Option<u32> {
    let norm = &app.norm;
    let name = &norm.name;
    if name == term {
        return Some(100);
    }
    if name.starts_with(term) {
        return Some(90);
    }
    if name.split_whitespace().any(|word| word.starts_with(term)) {
        return Some(80);
    }
    if name.contains(term) {
        return Some(70);
    }
    if norm.generic.split_whitespace().any(|word| word.starts_with(term)) {
        return Some(60);
    }
    if norm.generic.contains(term) {
        return Some(55);
    }
    if norm.keywords.iter().any(|k| k.starts_with(term)) {
        return Some(50);
    }
    if norm.keywords.iter().any(|k| k.contains(term)) {
        return Some(45);
    }
    if norm.executable.starts_with(term) {
        return Some(40);
    }
    if norm.id.contains(term) {
        return Some(35);
    }
    if norm.comment.contains(term) {
        return Some(20);
    }
    None
}

/// Quanto o app combina com a busca (maior é melhor), ou None. Busca vazia: Some(0).
/// A busca inteira primeiro; com várias palavras, cada uma precisa achar algo, como "editor texto".
pub fn score(app: &App, query: &str) -> Option<u32> {
    let query = normalize(query);
    if query.is_empty() {
        return Some(0);
    }
    if let Some(whole) = term_score(app, &query) {
        return Some(whole);
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.len() > 1 {
        let scores: Option<Vec<u32>> = words.iter().map(|word| term_score(app, word)).collect();
        return scores.and_then(|s| s.into_iter().min()).map(|min| min / 3);
    }
    None
}

/// Os apps que combinam, do melhor para o pior; empate, pela ordem de `sort_apps`.
pub fn search<'a>(apps: &'a [App], query: &str) -> Vec<&'a App> {
    let mut scored: Vec<(u32, &App)> = apps.iter().filter_map(|app| score(app, query).map(|s| (s, app))).collect();
    scored.sort_by(|(sa, a), (sb, b)| {
        (Reverse(*sa), &a.norm.name, &a.fields.id).cmp(&(Reverse(*sb), &b.norm.name, &b.fields.id))
    });
    scored.into_iter().map(|(_s, app)| app).collect()
}

// Contas na busca: só números, + - * / // % ** (ou ^) e parênteses, com a gramática do Python.
// O Python faz as contas de inteiros exatas; aqui tudo é f64, o que só muda acima de 2^53.

pub const MAX_POWER: f64 = 1000.0;
pub const MAX_CALC_LENGTH: usize = 200;
/// Parênteses e sinais aninhados; o Python recusa 200 parênteses, e a pilha não estoura
const MAX_NESTING: usize = 200;
/// A árvore é avaliada com recursão, e uma soma longa a deixa funda: como o RecursionError do
/// Python, uma conta com mais tokens que isso não é conta. Com MAX_CALC_LENGTH nunca chega aqui
const MAX_TOKENS: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    DoubleSlash,
    Percent,
    Power,
    Open,
    Close,
}

/// Os tokens como o tokenizador do Python os vê, ou None num erro de sintaxe.
fn tokenize(text: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' => i += 1,
            '0'..='9' | '.' => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let integer_digits = i - start;
                let mut is_float = false;
                if i < chars.len() && chars[i] == '.' {
                    is_float = true;
                    i += 1;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let literal: String = chars[start..i].iter().collect();
                if literal == "." || (integer_digits == 0 && !is_float) {
                    return None;
                }
                // 01 é erro de sintaxe no Python (00 e 01.5 não)
                if !is_float && literal.len() > 1 && literal.starts_with('0') && literal.chars().any(|d| d != '0') {
                    return None;
                }
                tokens.push(Token::Num(literal.parse().ok()?));
            }
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' if next == Some('*') => {
                tokens.push(Token::Power);
                i += 2;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' if next == Some('/') => {
                tokens.push(Token::DoubleSlash);
                i += 2;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            '(' => {
                tokens.push(Token::Open);
                i += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                i += 1;
            }
            _ => return None,
        }
    }
    Some(tokens)
}

#[derive(Debug)]
enum Expr {
    Num(f64),
    Neg(Box<Expr>),
    Pos(Box<Expr>),
    Bin(Token, Box<Expr>, Box<Expr>),
}

/// Descendente recursivo com a precedência do Python:
/// expr := term (('+'|'-') term)*
/// term := factor (('*'|'/'|'//'|'%') factor)*
/// factor := ('+'|'-') factor | power
/// power := atom ['**' factor]      (à direita: 2**3**2 = 512, -2**2 = -4, 2**-1 = 0.5)
/// atom := número | '(' expr ')'
struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.at).copied()
    }

    fn enter(&mut self) -> Option<()> {
        self.depth += 1;
        (self.depth <= MAX_NESTING).then_some(())
    }

    fn expr(&mut self) -> Option<Expr> {
        let mut left = self.term()?;
        while let Some(op @ (Token::Plus | Token::Minus)) = self.peek() {
            self.at += 1;
            left = Expr::Bin(op, Box::new(left), Box::new(self.term()?));
        }
        Some(left)
    }

    fn term(&mut self) -> Option<Expr> {
        let mut left = self.factor()?;
        while let Some(op @ (Token::Star | Token::Slash | Token::DoubleSlash | Token::Percent)) = self.peek() {
            self.at += 1;
            left = Expr::Bin(op, Box::new(left), Box::new(self.factor()?));
        }
        Some(left)
    }

    fn factor(&mut self) -> Option<Expr> {
        match self.peek() {
            Some(sign @ (Token::Plus | Token::Minus)) => {
                self.at += 1;
                self.enter()?;
                let operand = Box::new(self.factor()?);
                self.depth -= 1;
                Some(if sign == Token::Minus { Expr::Neg(operand) } else { Expr::Pos(operand) })
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Option<Expr> {
        let base = self.atom()?;
        if self.peek() == Some(Token::Power) {
            self.at += 1;
            self.enter()?;
            let exponent = self.factor()?;
            self.depth -= 1;
            return Some(Expr::Bin(Token::Power, Box::new(base), Box::new(exponent)));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<Expr> {
        match self.peek()? {
            Token::Num(value) => {
                self.at += 1;
                Some(Expr::Num(value))
            }
            Token::Open => {
                self.at += 1;
                self.enter()?;
                let inner = self.expr()?;
                self.depth -= 1;
                if self.peek() != Some(Token::Close) {
                    return None;
                }
                self.at += 1;
                Some(inner)
            }
            _ => None,
        }
    }
}

/// O resto com o sinal do divisor e a divisão inteira com piso, como o float do Python.
fn python_divmod(a: f64, b: f64) -> (f64, f64) {
    let mut modulo = a % b;
    let mut div = (a - modulo) / b;
    if modulo != 0.0 {
        if (b < 0.0) != (modulo < 0.0) {
            modulo += b;
            div -= 1.0;
        }
    } else {
        modulo = 0.0_f64.copysign(b);
    }
    let floordiv = if div != 0.0 {
        let mut floor = div.floor();
        if div - floor > 0.5 {
            floor += 1.0;
        }
        floor
    } else {
        0.0_f64.copysign(a / b)
    };
    (floordiv, modulo)
}

fn eval(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Num(value) => Some(*value),
        Expr::Neg(operand) => Some(-eval(operand)?),
        Expr::Pos(operand) => eval(operand),
        Expr::Bin(op, left, right) => {
            let (a, b) = (eval(left)?, eval(right)?);
            let value = match op {
                Token::Plus => a + b,
                Token::Minus => a - b,
                Token::Star => a * b,
                Token::Slash if b == 0.0 => return None,
                Token::Slash => a / b,
                Token::DoubleSlash | Token::Percent if b == 0.0 => return None,
                Token::DoubleSlash => python_divmod(a, b).0,
                Token::Percent => python_divmod(a, b).1,
                Token::Power => {
                    if b.abs() > MAX_POWER {
                        return None;
                    }
                    // 0 ** negativo é ZeroDivisionError; negativo ** fracionário é complex
                    if (a == 0.0 && b < 0.0) || (a < 0.0 && b.fract() != 0.0) {
                        return None;
                    }
                    a.powf(b)
                }
                _ => return None,
            };
            // Estouro (o OverflowError do Python) e infinito no meio da conta
            value.is_finite().then_some(value)
        }
    }
}

/// A conta já depois dos filtros de caracteres e de vírgula: o equivalente ao ast.parse e ao ev.
fn eval_text(text: &str) -> Option<f64> {
    let tokens = tokenize(&text.replace('^', "**"))?;
    if tokens.len() > MAX_TOKENS || !tokens.iter().any(|t| matches!(t, Token::Num(_))) {
        return None;
    }
    let mut parser = Parser { tokens, at: 0, depth: 0 };
    let expr = parser.expr()?;
    if parser.at != parser.tokens.len() {
        return None;
    }
    // Só número solto, mesmo com sinal ou parênteses, não é conta
    fn has_binary(expr: &Expr) -> bool {
        match expr {
            Expr::Num(_) => false,
            Expr::Neg(inner) | Expr::Pos(inner) => has_binary(inner),
            Expr::Bin(..) => true,
        }
    }
    if !has_binary(&expr) {
        return None;
    }
    eval(&expr).filter(|value| value.is_finite())
}

/// O resultado da conta, ou None se o texto não for uma conta. Só número solto não é conta.
pub fn calculate(text: &str) -> Option<f64> {
    let text = text.trim();
    let allowed = |c: char| c.is_ascii_digit() || ".,+-*/%^() \t".contains(c);
    if text.is_empty()
        || text.chars().count() > MAX_CALC_LENGTH
        || !text.chars().all(allowed)
        || !text.chars().any(|c| c.is_ascii_digit())
    {
        return None;
    }
    let text = if text.contains(',') {
        if text.contains('.') {
            return None; // 1.000,5 ou 1,000.5: ambíguo
        }
        text.replace(',', ".") // 2,5 em português
    } else {
        text.to_string()
    };
    eval_text(&text)
}

/// O que vai ao qalc, quando a conta simples não basta: uma conversão ("5 km to mi", "100 °F -> °C"), unidades
/// numa conta ("2 h + 30 min"), as duas começando por um número, ou uma função ("sqrt(2)"). Nada além disso: o
/// qalc lê qualquer palavra como unidade ou variável ("firefox" vira bytes, "mp3 to wav" também), e o "in" é
/// polegada. Os mesmos padrões do `sway_drawer.py`, presos ao começo do texto
static QALC_PATTERNS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [
        Regex::new(r"(?i)^[-+(]?\s*\d[^\n]*?(?:\s+to\s+|\s*(?:->|→)\s*)\S").unwrap(),
        Regex::new(r"^[-+(]?\s*\d[\d.,]*\s*[^\W\d_]+\s*[-+*/^×÷]\s*[\d(.]").unwrap(),
        Regex::new(r"(?i)^\b(?:sqrt|cbrt|sin|cos|tan|asin|acos|atan|log|ln|exp|abs|round|floor|ceil|factorial)\s*\(\s*[-+\d.(]")
            .unwrap(),
    ]
});

/// As funções do qalc que mexem fora da conta: o command() roda um programa pelo shell, e as outras leem ou
/// gravam arquivos ou abrem o gnuplot. A grade conta enquanto se digita, e o qalc fecha aspas e parênteses
/// abertos: um command() pela metade rodaria cortado. Com os nomes traduzidos que o qalc aceita
pub const QALC_DENY: [&str; 17] = [
    "command", "kommando", "команда", "запустить", "load", "lees", "ladda", "загрузить", "export", "экспорт", "save",
    "opslaan", "spara", "сохранить", "plot", "график", "диаграмма",
];

/// Se o texto tem cara de conversão ou de conta com unidades para o qalc. Um nome de app nunca tem.
pub fn qalc_candidate(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty()
        || text.chars().count() > MAX_CALC_LENGTH
        || text.contains('\n')
        || !text.chars().any(|c| c.is_numeric())
    {
        return false;
    }
    let lowered = text.to_lowercase();
    if QALC_DENY.iter().any(|name| lowered.contains(name)) {
        return false;
    }
    QALC_PATTERNS.iter().any(|pattern| pattern.is_match(text))
}

/// A sobra embaixo da grade para ela mostrar só linhas inteiras: com mais apps do que cabem, a última linha à
/// vista não sai cortada no meio (os ícones sem os nomes), e o resto vem rolando. `viewport` é a altura da
/// área que rola (contando a sobra de agora), `content` a altura de tudo o que rola e `bottoms` onde termina
/// cada item visível, medido no conteúdo: a sobra vai até o fim do último item que cabe inteiro. Cabendo tudo,
/// ou sem itens medidos, nenhuma.
pub fn row_gap(viewport: f64, content: f64, bottoms: &[f64]) -> f64 {
    if viewport <= 0.0 || content <= viewport {
        return 0.0;
    }
    bottoms.iter().copied().filter(|bottom| *bottom > 0.0 && *bottom <= viewport).fold(None, |a: Option<f64>, b| Some(a.map_or(b, |a| a.max(b))))
        .map_or(0.0, |bottom| viewport - bottom)
}

/// O resultado do `qalc -t` (a primeira linha), ou None se ele falhou ou não respondeu nada.
pub fn qalc_result(output: &str, success: bool) -> Option<String> {
    if !success {
        return None;
    }
    let line = output.trim().split('\n').next().unwrap_or("").trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Inteiro abaixo de 1e15 sem casas; senão, como o `{:.10g}` do Python. O ponto vira `separator`.
pub fn format_number(value: f64, separator: &str) -> String {
    let text = if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        general_format(value, 10)
    };
    text.replace('.', separator)
}

/// O `%g` do C e do Python com `precision` algarismos significativos.
fn general_format(value: f64, precision: usize) -> String {
    if !value.is_finite() {
        return format!("{value}");
    }
    let scientific = format!("{:.*e}", precision - 1, value);
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let strip = |text: &str| -> String {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text.to_string()
        }
    };
    if (-4..precision as i32).contains(&exponent) {
        let decimals = (precision as i32 - 1 - exponent) as usize;
        strip(&format!("{:.*}", decimals, value))
    } else {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", strip(mantissa), sign, exponent.abs())
    }
}

/// O que o .desktop pode pôr no Exec no lugar de arquivos e ícones; sem arquivo, sai
const FIELD_CODES: &[&str] = &["%f", "%F", "%u", "%U", "%d", "%D", "%n", "%N", "%i", "%c", "%k", "%v", "%m"];

/// O argv para abrir um app de terminal (Terminal=true) no terminal da sessão, ou None.
pub fn terminal_command(commandline: &str, terminal: &str) -> Option<Vec<String>> {
    let words: Vec<String> = shlex::split(commandline)?
        .into_iter()
        .filter(|word| !FIELD_CODES.contains(&word.as_str()))
        .map(|word| word.replace("%%", "%"))
        .collect();
    if words.is_empty() {
        return None;
    }
    let mut argv = vec![terminal.to_string(), "-e".to_string()];
    argv.extend(words);
    Some(argv)
}

/// O terminal dos apps com Terminal=true: o sway-terminal (o terminal da config), se existir; sem ele, o kitty
/// de antes, se instalado (`kitty`); senão None, e o Gio escolhe.
pub fn terminal_program(helper: Option<&Path>, kitty: bool) -> Option<String> {
    match helper {
        Some(path) => Some(path.to_string_lossy().into_owned()),
        None => kitty.then(|| "kitty".to_string()),
    }
}

/// Os fixados salvos. Sem o arquivo, os do nwg-drawer (legacy), se houver. Outro erro sobe:
/// tratar como lista vazia apagaria os fixados ao salvar. UTF-8 inválido também é erro.
pub fn load_pins(path: &Path, legacy: Option<&Path>) -> io::Result<Vec<String>> {
    for source in std::iter::once(path).chain(legacy) {
        let text = match fs::read_to_string(source) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        let mut pins: Vec<String> = Vec::new();
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let pin = line.strip_suffix(".desktop").unwrap_or(line).to_string();
            if !pins.contains(&pin) {
                pins.push(pin);
            }
        }
        return Ok(pins);
    }
    Ok(Vec::new())
}

pub fn save_pins(path: &Path, pins: &[String]) -> io::Result<()> {
    let text: String = pins.iter().map(|pin| format!("{pin}\n")).collect();
    swaydesk_common::text::atomic_write(path, text.as_bytes(), 0o600) // como o atomic_write do Python
}

pub fn toggle_pin(pins: &[String], key: &str) -> Vec<String> {
    if pins.iter().any(|pin| pin == key) {
        pins.iter().filter(|pin| *pin != key).cloned().collect()
    } else {
        let mut pins = pins.to_vec();
        pins.push(key.to_string());
        pins
    }
}

pub const MIN_FILE_QUERY: usize = 3;

/// Os limites da busca de arquivos.
#[derive(Clone, Copy, Debug)]
pub struct FileLimits {
    pub limit: usize,
    pub max_depth: usize,
    pub max_visited: usize,
}

impl Default for FileLimits {
    fn default() -> Self {
        FileLimits { limit: 20, max_depth: 6, max_visited: 30000 }
    }
}

/// As pastas do usuário onde procurar arquivos, resolvidas e sem repetir, sem a pessoal e sem uma
/// que esteja dentro de outra. A pessoal fica de fora: sem a pasta especial, o GLib devolve a
/// pessoal, que levaria tempo demais.
pub fn search_roots(home: &Path, special_dirs: &[Option<PathBuf>]) -> Vec<PathBuf> {
    let home = fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let mut roots: Vec<PathBuf> = Vec::new();
    for folder in special_dirs.iter().flatten() {
        let Ok(real) = fs::canonicalize(folder) else { continue };
        if real != home && real.is_dir() && !roots.contains(&real) {
            roots.push(real);
        }
    }
    // Uma dentro da outra apareceria duas vezes
    roots
        .iter()
        .filter(|r| !roots.iter().any(|o| *r != o && r.starts_with(o)))
        .cloned()
        .collect()
}

/// Arquivos e pastas cujo nome contém a busca, em largura: os mais rasos primeiro. (caminho, é pasta).
/// Sem ocultos e sem seguir links de pasta. Para no limite, no total visitado ou quando cancelled().
pub fn find_files(roots: &[PathBuf], query: &str, cancelled: &dyn Fn() -> bool, limits: FileLimits) -> Vec<(PathBuf, bool)> {
    let wanted = normalize(query);
    if wanted.chars().count() < MIN_FILE_QUERY {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut visited = 0;
    let mut level: Vec<(PathBuf, usize)> = roots.iter().map(|root| (root.clone(), 0)).collect();
    while !level.is_empty() {
        let mut following = Vec::new();
        for (folder, depth) in &level {
            if cancelled() {
                return found;
            }
            let Ok(entries) = fs::read_dir(folder) else { continue };
            let mut items: Vec<(String, fs::DirEntry)> = entries
                .flatten()
                .map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry))
                .collect();
            items.sort_by_cached_key(|(name, _entry)| name.to_lowercase());
            for (name, entry) in items {
                visited += 1;
                if visited > limits.max_visited {
                    return found;
                }
                if name.starts_with('.') {
                    continue;
                }
                let Ok(kind) = entry.file_type() else { continue };
                let is_dir = kind.is_dir(); // o file_type não segue links
                if normalize(&name).contains(&wanted) {
                    found.push((entry.path(), is_dir));
                    if found.len() >= limits.limit {
                        return found;
                    }
                }
                if is_dir && depth + 1 < limits.max_depth {
                    following.push((entry.path(), depth + 1));
                }
            }
        }
        level = following;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_the_length_limit_huge_or_deep_input_still_does_not_panic() {
        // O porte do teste com o MAX_CALC_LENGTH desligado: eval_text não tem o limite de tamanho
        let big = "9".repeat(90);
        assert_eq!(eval_text(&[big.as_str(); 4].join("*")), None); // grande demais para f64
        assert_eq!(eval_text(&format!("{}1+1{}", "(".repeat(5000), ")".repeat(5000))), None);
        assert_eq!(eval_text(&format!("{}1", "-".repeat(5000))), None);
        assert_eq!(eval_text(&["1"; 50000].join("+")), None); // o RecursionError do Python
        assert_eq!(eval_text(&["1"; 400].join("+")), Some(400.0));
    }

    #[test]
    fn python_division_and_remainder() {
        assert_eq!(python_divmod(-7.0, 2.0), (-4.0, 1.0));
        assert_eq!(python_divmod(7.0, -3.0), (-3.0, -2.0));
        assert_eq!(python_divmod(-7.0, 3.0), (-3.0, 2.0));
        assert_eq!(python_divmod(7.5, 2.0), (3.0, 1.5));
    }
}
