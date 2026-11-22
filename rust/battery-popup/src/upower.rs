//! O que o popup lê do upower, sem GTK: o porte das funções de `home/.local/bin/waybar-battery-popup`.
//! Os perfis de energia e a bateria da máquina ficam no `swaydesk_common::power`.
use std::collections::HashMap;

/// As horas do gráfico.
pub const HOURS: u32 = 12;

pub use swaydesk_common::power::sh;

/// Os campos do `upower -i`: a primeira ocorrência de cada chave vale.
pub fn parse_upower(text: &str) -> HashMap<String, String> {
    let mut found = HashMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            found.entry(key.trim().to_string()).or_insert_with(|| value.trim().to_string());
        }
    }
    found
}

/// O arredondamento do Python (`round`): o meio vai para o par.
pub fn py_round(x: f64) -> f64 {
    x.round_ties_even()
}

/// "3.2 hours" → "3 h 12 min"; "45.0 minutes" → "45 min". Vazio para mais de 48 h (o upower dá
/// estimativas absurdas com consumo quase zero); o texto como veio, se não for "número unidade".
pub fn duration(text: &str) -> String {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [n, unit] = parts.as_slice() else { return text.to_string() };
    let Ok(n) = n.parse::<f64>() else { return text.to_string() };
    let mins = if unit.starts_with("hour") {
        n * 60.0
    } else if unit.starts_with("day") {
        n * 1440.0
    } else {
        n
    };
    let mins = py_round(mins) as i64;
    if mins > 48 * 60 {
        return String::new();
    }
    if mins >= 60 {
        return if mins % 60 != 0 { format!("{} h {:02} min", mins / 60, mins % 60) } else { format!("{} h", mins / 60) };
    }
    format!("{mins} min")
}

/// O primeiro número do valor ("12.5 W", "87%").
pub fn num(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.trim_end_matches('%').parse().ok()
}

/// Os pontos (tempo, %) do `GetHistory` pelo `busctl --json=short`, sem os de estado 0, em ordem.
pub fn parse_history(json: &str) -> Vec<(f64, f64)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let Some(points) = value["data"][0].as_array() else { return Vec::new() };
    let mut found: Vec<(f64, f64)> = points
        .iter()
        .filter_map(|p| {
            let p = p.as_array()?;
            let (t, v, state) = (p.first()?.as_f64()?, p.get(1)?.as_f64()?, p.get(2)?.as_f64()?);
            (state != 0.0).then_some((t, v))
        })
        .collect();
    found.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    found
}

/// Os pontos das últimas HOURS horas. Resolução 0 derruba o upowerd.
pub fn history(path: &str) -> Vec<(f64, f64)> {
    let span = (HOURS * 3600).to_string();
    parse_history(&sh(&[
        "busctl", "--json=short", "--system", "call", "org.freedesktop.UPower", path,
        "org.freedesktop.UPower.Device", "GetHistory", "suu", "charge", &span, "200",
    ]))
}

/// Na tomada? Pelo sysfs, não pelo estado da bateria (que fica "pending" ao segurar a carga).
pub fn on_ac(root: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else { return false };
    entries.filter_map(|e| e.ok()).any(|e| {
        e.file_name().to_string_lossy().starts_with("AC")
            && std::fs::read_to_string(e.path().join("online")).is_ok_and(|v| v.trim() == "1")
    })
}

/// Os passos do gráfico: (x, y) em pixels, como o `Chart.on_draw` do Python. Vazio sem pontos.
pub fn chart_path(points: &[(f64, f64)], now_pct: Option<f64>, now: f64, w: f64, h: f64) -> Vec<(f64, f64)> {
    let pad = 3.0;
    let t0 = now - f64::from(HOURS) * 3600.0;
    let x = |t: f64| pad + (w - 2.0 * pad) * (t.max(t0) - t0) / (now - t0);
    let y = |v: f64| pad + (h - 2.0 * pad) * (1.0 - v / 100.0);
    let mut pts: Vec<(f64, f64)> = points.to_vec();
    if let Some(pct) = now_pct {
        pts.push((now, pct));
    }
    let before = pts.iter().rev().find(|p| p.0 <= t0).copied();
    let mut window: Vec<(f64, f64)> = before.map(|b| vec![(t0, b.1)]).unwrap_or_default();
    window.extend(pts.iter().filter(|p| p.0 > t0));
    let Some(&first) = window.first() else { return Vec::new() };
    // O upower só grava mudanças: sem amostra no início da janela, a carga já estava no primeiro valor
    if first.0 > t0 {
        window.insert(0, (t0, first.1));
    }
    let mut path = vec![(x(window[0].0), y(window[0].1))];
    for pair in window.windows(2) {
        let ((_, v0), (t1, v1)) = (pair[0], pair[1]);
        path.push((x(t1), y(v0)));
        path.push((x(t1), y(v1)));
    }
    path
}

/// A linha de y de um valor, para a grade.
pub fn chart_y(v: f64, h: f64) -> f64 {
    3.0 + (h - 6.0) * (1.0 - v / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UPOWER: &str = "  native-path:          BAT0
  vendor:               SMP
  model:                5B10W13930
  power supply:         yes
  battery
    present:             yes
    state:               discharging
    energy:              38.43 Wh
    energy-full:         49.68 Wh
    energy-full-design:  57 Wh
    energy-rate:         7.512 W
    time to empty:       5.1 hours
    percentage:          77%
    technology:          lithium-polymer
    charge-cycles:       N/A
  History (charge):
    1731400000\t77.000\tdischarging
";

    #[test]
    fn upower_fields() {
        let d = parse_upower(UPOWER);
        assert_eq!(d["state"], "discharging");
        assert_eq!(d["percentage"], "77%");
        assert_eq!(num(&d["percentage"]), Some(77.0));
        assert_eq!(num(&d["energy-rate"]), Some(7.512));
        assert_eq!(num("N/A"), None);
        assert_eq!(num(""), None);
        assert_eq!(d["model"], "5B10W13930");
    }

    #[test]
    fn durations_like_python() {
        assert_eq!(duration("3.2 hours"), "3 h 12 min");
        assert_eq!(duration("2.0 hours"), "2 h");
        assert_eq!(duration("45.0 minutes"), "45 min");
        assert_eq!(duration("3.0 days"), "");
        assert_eq!(duration("1.5 days"), "36 h");
        assert_eq!(duration("unknown"), "unknown");
        assert_eq!(duration(""), "");
        // round(0.5) == 0 e round(2.5) == 2 no Python
        assert_eq!(duration("0.5 minutes"), "0 min");
        assert_eq!(duration("2.5 minutes"), "2 min");
    }

    #[test]
    fn history_from_busctl_json() {
        let json = r#"{"type":"a(udu)","data":[[[1731400100,80.0,2],[1731400000,82.0,2],[1731400050,0.0,0]]]}"#;
        assert_eq!(parse_history(json), vec![(1731400000.0, 82.0), (1731400100.0, 80.0)]);
        assert!(parse_history("").is_empty());
        assert!(parse_history(r#"{"data":[]}"#).is_empty());
    }

    #[test]
    fn ac_from_sysfs() {
        let dir = std::env::temp_dir().join(format!("swaydesk-ac-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ACAD")).unwrap();
        std::fs::create_dir_all(dir.join("BAT0")).unwrap();
        std::fs::write(dir.join("BAT0/online"), "1\n").unwrap();
        std::fs::write(dir.join("ACAD/online"), "0\n").unwrap();
        assert!(!on_ac(&dir));
        std::fs::write(dir.join("ACAD/online"), "1\n").unwrap();
        assert!(on_ac(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn chart_steps() {
        let now = 100_000.0;
        let t0 = now - 43_200.0;
        // Sem pontos e sem a carga de agora, nada
        assert!(chart_path(&[], None, now, 106.0, 106.0).is_empty());
        // Só a carga de agora: uma linha reta do início ao fim (3 pontos: início, degrau, fim)
        let flat = chart_path(&[], Some(50.0), now, 106.0, 106.0);
        assert_eq!(flat, vec![(3.0, 53.0), (103.0, 53.0), (103.0, 53.0)]);
        // Um ponto antes da janela vale no início dela; o degrau sobe no meio
        let path = chart_path(&[(t0 - 10.0, 0.0), (t0 + 21_600.0, 100.0)], None, now, 106.0, 106.0);
        assert_eq!(path, vec![(3.0, 103.0), (53.0, 103.0), (53.0, 3.0)]);
        assert_eq!(chart_y(50.0, 106.0), 53.0);
    }
}
