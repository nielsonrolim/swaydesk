//! O painel Sobre: o que ele mostra, lido de arquivos que qualquer usuário lê (sem root nem polkit). Aqui só a
//! leitura do texto de cada fonte, com amostras nos testes; quem abre os arquivos é o app.

/// O nome da distro no /etc/os-release: o PRETTY_NAME, ou o NAME com a VERSION.
pub fn distro(os_release: &str) -> Option<String> {
    let value = |key: &str| {
        os_release.lines().find_map(|line| {
            let rest = line.trim().strip_prefix(key)?.strip_prefix('=')?;
            let unquoted = rest.trim().trim_matches(|c| c == '"' || c == '\'');
            (!unquoted.is_empty()).then(|| unquoted.to_string())
        })
    };
    value("PRETTY_NAME").or_else(|| {
        let name = value("NAME")?;
        Some(match value("VERSION") {
            Some(version) => format!("{name} {version}"),
            None => name,
        })
    })
}

/// A memória total do /proc/meminfo, em bytes.
pub fn mem_total(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find_map(|l| l.strip_prefix("MemTotal:"))?;
    let kib: u64 = line.split_whitespace().next()?.parse().ok()?;
    kib.checked_mul(1024)
}

/// O processador do /proc/cpuinfo e quantos núcleos lógicos ele mostra: "AMD Ryzen 7 7700 × 16". No ARM, sem o
/// "model name", vale o "Model" ou o "Hardware", que nomeiam a placa ("Raspberry Pi 4 Model B"): melhor que nada.
pub fn cpu(cpuinfo: &str) -> Option<String> {
    let field = |key: &str| {
        cpuinfo.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == key && !value.trim().is_empty()).then(|| value.trim().to_string())
        })
    };
    let model = field("model name").or_else(|| field("Model")).or_else(|| field("Hardware"))?;
    let cores = cpuinfo.lines().filter(|l| l.split_once(':').is_some_and(|(n, _)| n.trim() == "processor")).count();
    Some(if cores > 1 { format!("{model} × {cores}") } else { model })
}

/// A versão do Sway na saída do `swaymsg -t get_version` (o human_readable).
pub fn sway_version(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value.get("human_readable")?.as_str().filter(|v| !v.is_empty()).map(str::to_string)
}

/// O nome de uma placa de vídeo no pci.ids, pelos ids do /sys/class/drm/cardN/device (vendor e device, "0x1002").
pub fn gpu_name(vendor: &str, device: &str, pci_ids: &str) -> Option<String> {
    let id = |text: &str| text.trim().trim_start_matches("0x").to_ascii_lowercase();
    let (vendor, device) = (id(vendor), id(device));
    let mut lines = pci_ids.lines().skip_while(|line| !line.starts_with(&format!("{vendor}  ")));
    let vendor_name = lines.next()?[vendor.len()..].trim().to_string();
    // O nome curto entre colchetes ("AMD/ATI"), se houver
    let short = match (vendor_name.rfind('['), vendor_name.ends_with(']')) {
        (Some(start), true) => vendor_name[start + 1..vendor_name.len() - 1].to_string(),
        _ => vendor_name,
    };
    lines
        .take_while(|line| line.starts_with('\t') || line.starts_with('#') || line.is_empty())
        .filter(|line| !line.starts_with("\t\t"))
        .find_map(|line| line.strip_prefix('\t')?.strip_prefix(&format!("{device}  ")))
        .map(|name| format!("{short} {}", name.trim()))
}

/// As placas na ordem em que apareceram, as iguais juntas com a conta: ["A", "A", "B"] vira ["A × 2", "B"].
pub fn count_same(names: &[String]) -> Vec<String> {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for name in names {
        match seen.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => seen.push((name, 1)),
        }
    }
    seen.into_iter().map(|(name, count)| if count > 1 { format!("{name} × {count}") } else { name.to_string() }).collect()
}

/// Um tamanho em bytes para gente: "15.5 GiB".
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// O que o painel mostra, linha por linha; o que não deu para ler fica de fora.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct About {
    pub swaydesk: Option<String>,
    pub distro: Option<String>,
    pub kernel: Option<String>,
    pub sway: Option<String>,
    pub cpu: Option<String>,
    /// Em bytes
    pub memory: Option<u64>,
    pub gpus: Vec<String>,
    /// O disco do /, em bytes: (usado, total)
    pub disk: Option<(u64, u64)>,
    pub hostname: Option<String>,
}

/// O texto do "Copiar informações", para colar num relato de bug: uma linha "Nome: valor" por item, com os nomes
/// em inglês (quem lê o relato é o projeto), na ordem do painel. Sem o nome da máquina: o relato costuma ser
/// público, e o nome dela muitas vezes é o da pessoa.
pub fn report(about: &About) -> String {
    let mut out = String::new();
    let mut line = |name: &str, value: &Option<String>| {
        if let Some(value) = value {
            // As contas em ASCII ("× 16" vira "x 16"): colado de um text/plain sem charset, o GTK 4 escapa o ×
            out.push_str(&format!("{name}: {}\n", value.replace(" × ", " x ")));
        }
    };
    line("Swaydesk", &about.swaydesk);
    line("Distribution", &about.distro);
    line("Kernel", &about.kernel);
    line("Sway", &about.sway);
    line("CPU", &about.cpu);
    line("Memory", &about.memory.map(human_size));
    for gpu in &about.gpus {
        line("GPU", &Some(gpu.clone()));
    }
    line("Disk (/)", &about.disk.map(|(used, total)| format!("{} used of {}", human_size(used), human_size(total))));
    out
}

/// O texto do "Copiar informações": o relato sem a quebra de linha do fim, que ficava sobrando na colagem.
pub fn copy_text(about: &About) -> String {
    let mut text = report(about);
    if text.ends_with('\n') {
        text.pop();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const OS_RELEASE: &str = "PRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\nNAME=\"Debian GNU/Linux\"\nVERSION_ID=\"13\"\n";

    #[test]
    fn distro_prefers_the_pretty_name_and_unquotes_it() {
        assert_eq!(distro(OS_RELEASE).as_deref(), Some("Debian GNU/Linux 13 (trixie)"));
        assert_eq!(distro("NAME=Arch Linux\nID=arch\n").as_deref(), Some("Arch Linux"));
        assert_eq!(distro("NAME='Foo'\nVERSION='2 (bar)'\n").as_deref(), Some("Foo 2 (bar)"));
        assert_eq!(distro("# só comentário\nID=x\n"), None);
    }

    #[test]
    fn mem_total_reads_kib_into_bytes() {
        let meminfo = "MemTotal:       32768000 kB\nMemFree:         1000 kB\n";
        assert_eq!(mem_total(meminfo), Some(32_768_000 * 1024));
        assert_eq!(mem_total("MemFree: 1 kB\n"), None);
    }

    #[test]
    fn cpu_names_the_model_and_counts_the_logical_cores() {
        let cpuinfo = "processor\t: 0\nmodel name\t: AMD Ryzen 7 7700 8-Core Processor\n\nprocessor\t: 1\nmodel name\t: AMD Ryzen 7 7700 8-Core Processor\n";
        assert_eq!(cpu(cpuinfo).as_deref(), Some("AMD Ryzen 7 7700 8-Core Processor × 2"));
        // ARM: sem "model name", o "Model" da placa
        let arm = "processor\t: 0\nBogoMIPS\t: 108.00\n\nprocessor\t: 1\n\nModel\t\t: Raspberry Pi 4 Model B Rev 1.4\n";
        assert_eq!(cpu(arm).as_deref(), Some("Raspberry Pi 4 Model B Rev 1.4 × 2"));
        assert_eq!(cpu(""), None);
    }

    #[test]
    fn sway_version_is_the_human_readable_one() {
        let json = r#"{"human_readable": "1.11", "variant": "sway", "major": 1, "minor": 11, "patch": 0, "loaded_config_file_name": "/x"}"#;
        assert_eq!(sway_version(json).as_deref(), Some("1.11"));
        assert_eq!(sway_version("not json"), None);
    }

    #[test]
    fn gpu_name_looks_the_ids_up_in_pci_ids() {
        let ids = "# comentário\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t7590  Navi 44 [Radeon RX 9060 XT]\n\t\t1002 0001  Sub\n\t7591  Other\n10de  NVIDIA Corporation\n\t2684  AD102 [GeForce RTX 4090]\n8086  Intel Corporation\n";
        assert_eq!(gpu_name("0x1002", "0x7590", ids).as_deref(), Some("AMD/ATI Navi 44 [Radeon RX 9060 XT]"));
        assert_eq!(gpu_name("0x10de\n", "0x2684\n", ids).as_deref(), Some("NVIDIA Corporation AD102 [GeForce RTX 4090]"));
        // Um device de outro vendor com o mesmo número não conta
        assert_eq!(gpu_name("0x8086", "0x7590", ids), None);
        assert_eq!(gpu_name("0x1002", "0x9999", ids), None);
    }

    #[test]
    fn identical_cards_are_counted_not_merged() {
        let names: Vec<String> = ["A", "B", "A"].iter().map(|s| s.to_string()).collect();
        assert_eq!(count_same(&names), ["A × 2", "B"]);
        assert_eq!(count_same(&["A".to_string()]), ["A"]);
        assert!(count_same(&[]).is_empty());
    }

    #[test]
    fn human_size_uses_binary_units() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(16 * 1024 * 1024 * 1024), "16.0 GiB");
        assert_eq!(human_size(1536 * 1024 * 1024), "1.5 GiB");
        assert_eq!(human_size(2 * 1024u64.pow(4)), "2.0 TiB");
    }

    #[test]
    fn report_has_one_line_per_known_item_in_panel_order() {
        let about = About {
            swaydesk: Some("0.2.0".into()),
            distro: Some("Arch Linux".into()),
            kernel: Some("6.12.1-arch1-1".into()),
            sway: None,
            memory: Some(16 * 1024u64.pow(3)),
            gpus: vec!["A".into(), "B".into()],
            disk: Some((10 * 1024u64.pow(3), 100 * 1024u64.pow(3))),
            hostname: Some("pc".into()),
            ..About::default()
        };
        assert_eq!(
            report(&about),
            "Swaydesk: 0.2.0\nDistribution: Arch Linux\nKernel: 6.12.1-arch1-1\nMemory: 16.0 GiB\nGPU: A\nGPU: B\n\
             Disk (/): 10.0 GiB used of 100.0 GiB\n"
        );
        // O relato vai para bugs públicos: o nome da máquina (muitas vezes o da pessoa) fica de fora
        assert!(!report(&about).contains("pc\n"));
    }

    #[test]
    fn the_report_writes_the_counts_in_ascii() {
        // O text/plain sem charset do GTK 4 escapa o que não é ASCII ("\\C3\\97" no lugar do ×)
        let about = About { cpu: Some("Ryzen 7 × 16".into()), gpus: vec!["A × 2".into()], ..About::default() };
        assert_eq!(report(&about), "CPU: Ryzen 7 x 16\nGPU: A x 2\n");
        assert!(report(&about).is_ascii());
    }

    #[test]
    fn the_copied_text_is_the_report_without_the_last_newline() {
        let about = About { kernel: Some("6.12".into()), memory: Some(1024), ..About::default() };
        assert_eq!(copy_text(&about), "Kernel: 6.12\nMemory: 1.0 KiB");
        assert_eq!(format!("{}\n", copy_text(&about)), report(&about));
    }
}
