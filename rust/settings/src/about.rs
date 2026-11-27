//! O painel Sobre: o Swaydesk, a distro e o hardware, tudo lido de arquivos que o usuário lê (sem root nem
//! polkit), e os botões Copiar informações, Ajuda e Site. A leitura do texto é do swaydesk-settings-logic; aqui
//! só se abrem os arquivos, numa thread (o swaymsg pode demorar), e se monta a página.
use std::path::Path;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use swaydesk_common::i18n::{ptr, tr};
use swaydesk_settings_logic::about::{self, About};

const SITE: &str = "https://swaydesk.org";
/// O pci.ids: no Debian e no Ubuntu (o pacote pci.ids) e no Arch (o hwdata)
const PCI_IDS: &[&str] = &["/usr/share/misc/pci.ids", "/usr/share/hwdata/pci.ids"];

fn read(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn first_line(path: &str) -> Option<String> {
    read(path).map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// As placas de vídeo do /sys/class/drm (cardN, sem os conectores cardN-DP-1): o nome do pci.ids, ou os ids e o
/// driver quando o pci.ids não está instalado ou não as conhece.
fn gpus() -> Vec<String> {
    let ids = PCI_IDS.iter().find_map(read).unwrap_or_default();
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else { return Vec::new() };
    let mut cards: Vec<_> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.strip_prefix("card").is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())))
        .collect();
    cards.sort();
    let mut names: Vec<String> = Vec::new(); // uma por placa: as iguais são contadas no fim
    for card in cards {
        let device = Path::new("/sys/class/drm").join(&card).join("device");
        let (Some(vendor), Some(product)) = (read(device.join("vendor")), read(device.join("device"))) else { continue };
        let name = about::gpu_name(&vendor, &product, &ids).unwrap_or_else(|| {
            let driver = std::fs::read_link(device.join("driver"))
                .ok()
                .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()));
            let id = format!("{}:{}", vendor.trim().trim_start_matches("0x"), product.trim().trim_start_matches("0x"));
            match driver {
                Some(driver) => format!("{id} ({driver})"),
                None => id,
            }
        });
        names.push(name);
    }
    about::count_same(&names)
}

/// O disco do /: (usado, total) em bytes.
fn disk() -> Option<(u64, u64)> {
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: caminho terminado em NUL e um statvfs válido para o kernel preencher
    if unsafe { libc::statvfs(c"/".as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let block = stat.f_frsize as u64;
    let total = (stat.f_blocks as u64).checked_mul(block)?;
    let free = (stat.f_bfree as u64).checked_mul(block)?;
    Some((total.saturating_sub(free), total))
}

/// A versão do Swaydesk: a dos pacotes (a mesma do `--version` de todo binário).
fn swaydesk_version() -> Option<String> {
    Some(swaydesk_common::cli::VERSION.to_string())
}

/// Tudo o que o painel mostra. Lento (o swaymsg): fora da linha principal.
fn collect() -> About {
    About {
        swaydesk: swaydesk_version(),
        distro: read("/etc/os-release").or_else(|| read("/usr/lib/os-release")).and_then(|t| about::distro(&t)),
        kernel: first_line("/proc/sys/kernel/osrelease"),
        sway: about::sway_version(&swaydesk_ui::cmd::status("swaymsg", &["-t", "get_version"])),
        cpu: read("/proc/cpuinfo").and_then(|t| about::cpu(&t)),
        memory: read("/proc/meminfo").and_then(|t| about::mem_total(&t)),
        gpus: gpus(),
        disk: disk(),
        hostname: first_line("/proc/sys/kernel/hostname"),
    }
}

/// O texto do "Copiar informações" (e do `--about`).
pub fn report_text() -> String {
    about::report(&collect())
}

/// As linhas da página, com os nomes traduzidos (o relato copiado fica em inglês).
fn rows(info: &About) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let mut add = |name: String, value: Option<String>| {
        if let Some(value) = value {
            rows.push((name, value));
        }
    };
    add(String::from("Swaydesk"), info.swaydesk.clone());
    add(tr("Distribution"), info.distro.clone());
    add(tr("Kernel"), info.kernel.clone());
    add(String::from("Sway"), info.sway.clone());
    add(tr("Processor"), info.cpu.clone());
    add(tr("Memory"), info.memory.map(about::human_size));
    for gpu in &info.gpus {
        add(tr("Graphics"), Some(gpu.clone()));
    }
    add(
        tr("Disk"),
        info.disk.map(|(used, total)| {
            tr("%(used)s used of %(total)s").replace("%(used)s", &about::human_size(used)).replace("%(total)s", &about::human_size(total))
        }),
    );
    add(tr("Device name"), info.hostname.clone());
    rows
}

fn fill(grid: &gtk4::Grid, info: &About) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
    if rows(info).is_empty() {
        let none = gtk4::Label::new(Some(&tr("Could not read the system information")));
        none.add_css_class("dim-label");
        grid.attach(&none, 0, 0, 2, 1);
        return;
    }
    for (row, (name, value)) in rows(info).into_iter().enumerate() {
        let key = gtk4::Label::new(Some(&name));
        key.set_xalign(1.0);
        key.add_css_class("dim-label");
        let value = gtk4::Label::new(Some(&value));
        value.set_xalign(0.0);
        value.set_selectable(true);
        value.set_wrap(true);
        grid.attach(&key, 0, row as i32, 1, 1);
        grid.attach(&value, 1, row as i32, 1, 1);
    }
}

fn help_program() -> Option<std::path::PathBuf> {
    Some(swaydesk_common::paths::component("sway-help", "swaydesk-help")).filter(|p| p.is_file())
}

pub fn page() -> gtk4::Widget {
    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 18);
    page.set_margin_top(36);
    page.set_margin_bottom(24);
    page.set_margin_start(24);
    page.set_margin_end(24);
    page.set_halign(gtk4::Align::Center);
    let logo = gtk4::Image::from_icon_name("preferences-system");
    logo.set_pixel_size(96);
    let title = gtk4::Label::new(Some("Swaydesk"));
    title.add_css_class("title-1");
    let grid = gtk4::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(18);
    grid.set_halign(gtk4::Align::Center);
    let loading = gtk4::Label::new(Some("…"));
    grid.attach(&loading, 0, 0, 2, 1);

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::Center);
    let copy = gtk4::Button::with_label(&tr("Copy info"));
    copy.set_sensitive(false);
    buttons.append(&copy);
    if let Some(program) = help_program() {
        let help = gtk4::Button::with_label(&ptr("button", "Help"));
        help.connect_clicked(move |_| {
            if let Err(err) = swaydesk_ui::cmd::spawn_app(&program, &[] as &[&str]) {
                eprintln!("swaydesk-settings: {}: {err}", program.display());
            }
        });
        buttons.append(&help);
    }
    let site = gtk4::Button::with_label(&tr("Website"));
    site.connect_clicked(|_| {
        if let Err(err) = gio::AppInfo::launch_default_for_uri(SITE, None::<&gio::AppLaunchContext>) {
            eprintln!("swaydesk-settings: {SITE}: {err}");
        }
    });
    buttons.append(&site);

    page.append(&logo);
    page.append(&title);
    page.append(&grid);
    page.append(&buttons);

    let weak_grid = grid.downgrade();
    let weak_copy = copy.downgrade();
    glib::spawn_future_local(async move {
        let info = match gio::spawn_blocking(collect).await {
            Ok(info) => info,
            Err(_) => {
                eprintln!("swaydesk-settings: reading the system information failed (panic)");
                About::default()
            }
        };
        let (Some(grid), Some(copy)) = (weak_grid.upgrade(), weak_copy.upgrade()) else { return };
        fill(&grid, &info);
        let text = about::copy_text(&info);
        copy.connect_clicked(move |button| {
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_text(&text);
            }
            button.set_label(&tr("Copied"));
            let weak = button.downgrade();
            glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                if let Some(button) = weak.upgrade() {
                    button.set_label(&tr("Copy info"));
                }
            });
        });
        // Sem nada lido, não há o que copiar
        copy.set_sensitive(!rows(&info).is_empty());
    });
    page.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_starts_with_the_swaydesk_version() {
        let info = About { swaydesk: Some("0.2.0".into()), kernel: Some("6.12".into()), ..About::default() };
        assert_eq!(rows(&info).first(), Some(&(String::from("Swaydesk"), String::from("0.2.0"))));
    }

    #[test]
    fn the_version_is_the_packages_one() {
        assert_eq!(swaydesk_version().as_deref(), Some(swaydesk_common::cli::VERSION));
    }
}
