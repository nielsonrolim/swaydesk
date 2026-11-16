//! A janela: o mapa das telas (arrastar para posicionar, com encaixe nas bordas), os ajustes da tela
//! escolhida e o Aplicar, com a pergunta "Manter ou desfazer" que volta sozinha em 15 s.
use std::cell::{Cell, RefCell};
use std::fs::File;
use std::rc::{Rc, Weak};

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use swaydesk_common::config;
use swaydesk_common::i18n::tr;
use swaydesk_displays_logic::conf;
use swaydesk_displays_logic::config::{commands, OutputConfig, Transform, SCALE_MAX, SCALE_MIN};
use swaydesk_displays_logic::geometry::{self, Rect};
use swaydesk_displays_logic::outputs::{self, Mode, Output};
use swaydesk_displays_logic::profiles::{self, Profile, Profiles};

use crate::{identify, sway, take_preview_lock, warn};

/// Quanto tempo a prévia espera antes de voltar sozinha
const CONFIRM_SECONDS: u32 = 15;
/// O aviso por cima do mapa: escuro e translúcido, legível no tema claro e no escuro
const CSS: &str = ".displays-info { background-color: rgba(0, 0, 0, 0.75); color: white; border-radius: 8px; padding: 6px 12px; }";
/// A margem do mapa e a distância de encaixe, em pixels da tela do app
const MAP_MARGIN: f64 = 24.0;
const SNAP_PIXELS: f64 = 12.0;

/// A área de trabalho guardada no Aplicar (lida uma vez: o Desfazer a usa e a esquece)
fn w_home(me: &Weak<Window>) -> Option<(String, String)> {
    me.upgrade().and_then(|w| w.home.borrow_mut().take())
}

/// Cada saída ligada ou desligada, para conferir depois de aplicar
fn switches(configs: &[(String, OutputConfig)]) -> Vec<(String, bool)> {
    configs.iter().map(|(name, config)| (name.clone(), config.enabled)).collect()
}

fn transform_label(t: Transform) -> String {
    match t {
        Transform::Normal => tr("Normal"),
        Transform::R90 => "90°".into(),
        Transform::R180 => "180°".into(),
        Transform::R270 => "270°".into(),
        Transform::Flipped => tr("Flipped"),
        Transform::Flipped90 => format!("{} 90°", tr("Flipped")),
        Transform::Flipped180 => format!("{} 180°", tr("Flipped")),
        Transform::Flipped270 => format!("{} 270°", tr("Flipped")),
    }
}

/// As resoluções da saída, sem repetir, da maior para a menor
fn resolutions(output: &Output) -> Vec<(i32, i32)> {
    let mut list: Vec<(i32, i32)> = Vec::new();
    for mode in &output.modes {
        if !list.contains(&(mode.width, mode.height)) {
            list.push((mode.width, mode.height));
        }
    }
    list
}

/// As taxas de uma resolução, da maior para a menor
fn rates(output: &Output, size: (i32, i32)) -> Vec<Mode> {
    output.modes.iter().filter(|m| (m.width, m.height) == size).copied().collect()
}

fn hex_rgba(hex: &str, alpha: f32) -> gdk::RGBA {
    let mut color = gdk::RGBA::parse(hex).unwrap_or(gdk::RGBA::new(0.2, 0.5, 0.8, 1.0));
    color.set_alpha(alpha);
    color
}

/// Troca as opções de um DropDown só se mudaram: um modelo novo refaz a lista e a seleção
fn set_choices(dropdown: &gtk4::DropDown, labels: &[String]) {
    let current: Vec<String> = dropdown
        .model()
        .and_downcast::<gtk4::StringList>()
        .map(|list| (0..list.n_items()).filter_map(|i| list.string(i)).map(|s| s.to_string()).collect())
        .unwrap_or_default();
    if current != labels {
        dropdown.set_model(Some(&gtk4::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>())));
    }
}

fn set_selected(dropdown: &gtk4::DropDown, index: usize) {
    if dropdown.selected() != index as u32 {
        dropdown.set_selected(index as u32);
    }
}

/// O mapa em pixels: escala e deslocamento
#[derive(Clone, Copy)]
struct View {
    scale: f64,
    ox: f64,
    oy: f64,
}

impl View {
    fn to_screen(&self, r: &Rect) -> (f64, f64, f64, f64) {
        (self.ox + r.x as f64 * self.scale, self.oy + r.y as f64 * self.scale, r.w as f64 * self.scale, r.h as f64 * self.scale)
    }
}

struct Drag {
    index: usize,
    start: (i32, i32),
    view: View,
}

pub struct Window {
    pub window: gtk4::ApplicationWindow,
    me: Weak<Window>,
    area: gtk4::DrawingArea,
    screens: gtk4::DropDown,
    enabled: gtk4::Switch,
    resolution: gtk4::DropDown,
    rate: gtk4::DropDown,
    scale: gtk4::SpinButton,
    scale_info: gtk4::Label,
    rotation: gtk4::DropDown,
    info: gtk4::Label,
    apply_button: gtk4::Button,
    reset_button: gtk4::Button,
    outputs: RefCell<Vec<Output>>,
    edits: RefCell<Vec<OutputConfig>>,
    selected: Cell<usize>,
    /// Preenchendo os campos: os sinais deles não são escolhas do usuário
    updating: Cell<bool>,
    drag: RefCell<Option<Drag>>,
    view: Cell<Option<View>>,
    /// A trava da prévia, enquanto o "Manter ou desfazer" está aberto
    preview: RefCell<Option<File>>,
    /// A área de trabalho do app e a tela dela antes do Aplicar: o Desfazer a leva de volta
    home: RefCell<Option<(String, String)>>,
    accent: String,
}

impl Window {
    #[allow(deprecated)]
    pub fn new(app: &gtk4::Application) -> Rc<Window> {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(CSS);
        if let Some(display) = gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        let window = gtk4::ApplicationWindow::new(app);
        window.set_title(Some(&tr("Displays")));
        window.set_default_size(780, 600);
        let accent = config::palette(&config::load()).accent;

        let area = gtk4::DrawingArea::new();
        area.set_vexpand(true);
        area.set_content_height(260);
        let frame = gtk4::Frame::new(None);
        frame.set_child(Some(&area));

        let screens = gtk4::DropDown::from_strings(&[]);
        screens.set_hexpand(true);
        let enabled = gtk4::Switch::new();
        enabled.set_valign(gtk4::Align::Center);
        let enabled_label = gtk4::Label::new(Some(&tr("On")));
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        top.append(&screens);
        top.append(&enabled_label);
        top.append(&enabled);

        let resolution = gtk4::DropDown::from_strings(&[]);
        let rate = gtk4::DropDown::from_strings(&[]);
        let scale = gtk4::SpinButton::with_range(SCALE_MIN, SCALE_MAX, 0.05);
        scale.set_digits(2);
        scale.set_increments(0.05, 0.25);
        let scale_info = gtk4::Label::new(None);
        scale_info.add_css_class("dim-label");
        scale_info.set_xalign(0.0);
        let labels: Vec<String> = Transform::ALL.iter().map(|t| transform_label(*t)).collect();
        let rotation = gtk4::DropDown::from_strings(&labels.iter().map(String::as_str).collect::<Vec<_>>());

        let grid = gtk4::Grid::new();
        grid.set_row_spacing(8);
        grid.set_column_spacing(12);
        let rows: [(String, &gtk4::Widget); 4] = [
            (tr("Resolution"), resolution.upcast_ref()),
            (tr("Refresh rate"), rate.upcast_ref()),
            (tr("Scale"), scale.upcast_ref()),
            (tr("Rotation"), rotation.upcast_ref()),
        ];
        for (row, (text, widget)) in rows.iter().enumerate() {
            let label = gtk4::Label::new(Some(text));
            label.set_xalign(0.0);
            grid.attach(&label, 0, row as i32, 1, 1);
            widget.set_hexpand(true);
            grid.attach(*widget, 1, row as i32, 1, 1);
        }
        grid.attach(&scale_info, 2, 2, 1, 1);

        // O aviso fica por cima do mapa, embaixo: aparecer e sumir não empurra os controles (um clique já mirado
        // não cai no lugar errado)
        let info = gtk4::Label::new(None);
        info.set_wrap(true);
        info.set_justify(gtk4::Justification::Center);
        info.set_visible(false);
        info.add_css_class("displays-info");
        info.set_halign(gtk4::Align::Center);
        info.set_valign(gtk4::Align::End);
        info.set_margin_bottom(10);
        info.set_margin_start(16);
        info.set_margin_end(16);
        info.set_can_target(false);
        let map = gtk4::Overlay::new();
        map.set_child(Some(&frame));
        map.add_overlay(&info);

        let identify_button = gtk4::Button::with_label(&tr("Identify"));
        identify_button.set_tooltip_text(Some(&tr("Show the name of each screen on it")));
        let reset_button = gtk4::Button::with_label(&tr("Undo changes"));
        let apply_button = gtk4::Button::with_label(&tr("Apply"));
        apply_button.add_css_class("suggested-action");
        let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        actions.append(&identify_button);
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        actions.append(&spacer);
        actions.append(&reset_button);
        actions.append(&apply_button);

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 14);
        for side in [gtk4::PositionType::Top, gtk4::PositionType::Bottom, gtk4::PositionType::Left, gtk4::PositionType::Right] {
            match side {
                gtk4::PositionType::Top => content.set_margin_top(16),
                gtk4::PositionType::Bottom => content.set_margin_bottom(16),
                gtk4::PositionType::Left => content.set_margin_start(16),
                _ => content.set_margin_end(16),
            }
        }
        let hint = gtk4::Label::new(Some(&tr("Drag the screens to arrange them. Each set of monitors remembers its own layout.")));
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        hint.add_css_class("dim-label");
        content.append(&hint);
        content.append(&map);
        content.append(&top);
        content.append(&grid);
        content.append(&actions);
        // Numa tela pequena (ou com escala alta) a janela não cabe: o conteúdo rola, sem cortar os botões
        let scroller = gtk4::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scroller.set_propagate_natural_height(true);
        scroller.set_child(Some(&content));
        window.set_child(Some(&scroller));

        let me = Rc::new_cyclic(|me| Window {
            window,
            me: me.clone(),
            area,
            screens,
            enabled,
            resolution,
            rate,
            scale,
            scale_info,
            rotation,
            info,
            apply_button,
            reset_button: reset_button.clone(),
            outputs: RefCell::new(Vec::new()),
            edits: RefCell::new(Vec::new()),
            selected: Cell::new(0),
            updating: Cell::new(false),
            drag: RefCell::new(None),
            view: Cell::new(None),
            preview: RefCell::new(None),
            home: RefCell::new(None),
            accent,
        });
        me.connect(&identify_button);
        me.reload();
        me.watch();
        me.window.present();
        // O foco começa na lista das telas: o Tab segue para os ajustes da escolhida
        me.screens.grab_focus();
        me
    }

    /// Abrir de novo (a janela já estava aberta): traz a janela e escolhe a tela com o foco, se não houver
    /// mudança por aplicar nem uma prévia na tela
    pub fn reopen(&self) {
        if self.preview.borrow().is_none() && self.drag.borrow().is_none() && !self.dirty() {
            if let Some(list) = sway::get_outputs() {
                let focused = list.iter().position(|o| o.focused);
                if list.iter().map(|o| &o.name).eq(self.outputs.borrow().iter().map(|o| &o.name)) {
                    if let Some(index) = focused {
                        self.select(index);
                    }
                }
            }
        }
        self.window.present();
    }

    fn with<A>(&self, f: impl Fn(&Rc<Window>, A) + 'static) -> impl Fn(A) + 'static {
        let me = self.me.clone();
        move |a| {
            if let Some(w) = me.upgrade() {
                f(&w, a)
            }
        }
    }

    fn connect(self: &Rc<Self>, identify_button: &gtk4::Button) {
        let me = self.me.clone();
        self.area.set_draw_func(move |area, cr, w, h| {
            if let Some(win) = me.upgrade() {
                win.draw(area, cr, w as f64, h as f64);
            }
        });
        let drag = gtk4::GestureDrag::new();
        let me = self.me.clone();
        drag.connect_drag_begin(move |_, x, y| {
            if let Some(w) = me.upgrade() {
                w.drag_begin(x, y);
            }
        });
        let me = self.me.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            if let Some(w) = me.upgrade() {
                w.drag_update(dx, dy);
            }
        });
        let me = self.me.clone();
        drag.connect_drag_end(move |_, _, _| {
            if let Some(w) = me.upgrade() {
                w.drag_end();
            }
        });
        self.area.add_controller(drag);

        // Os sinais dos campos: só as escolhas do usuário, não o preenchimento. A resposta vem depois do
        // sinal (idle): trocar o modelo de um GtkDropDown dentro do notify::selected dele mesmo, com o
        // popover aberto, prende o GTK num laço (visto na VM ao escolher a resolução)
        fn later(me: &Weak<Window>, f: impl Fn(&Rc<Window>) + 'static) -> impl Fn() + 'static {
            let me = me.clone();
            let f = Rc::new(f);
            move || {
                let Some(w) = me.upgrade() else { return };
                if w.updating.get() {
                    return;
                }
                let (me, f) = (me.clone(), f.clone());
                glib::idle_add_local_once(move || {
                    if let Some(w) = me.upgrade() {
                        f(&w);
                    }
                });
            }
        }
        // O valor é lido de novo na hora (no idle): se a lista mudou nesse meio tempo, vale a escolha atual
        let f = later(&self.me, |w| w.select(w.screens.selected() as usize));
        self.screens.connect_selected_notify(move |_| f());
        let f = later(&self.me, |w| w.set_enabled(w.enabled.is_active()));
        self.enabled.connect_active_notify(move |_| f());
        let f = later(&self.me, |w| w.set_resolution(w.resolution.selected() as usize));
        self.resolution.connect_selected_notify(move |_| f());
        let f = later(&self.me, |w| w.set_rate(w.rate.selected() as usize));
        self.rate.connect_selected_notify(move |_| f());
        let f = later(&self.me, |w| w.set_scale(w.scale.value()));
        self.scale.connect_value_changed(move |_| f());
        let f = later(&self.me, |w| {
            w.set_transform(Transform::ALL.get(w.rotation.selected() as usize).copied().unwrap_or_default())
        });
        self.rotation.connect_selected_notify(move |_| f());
        let f = self.with(|w, ()| w.reset());
        self.reset_button.connect_clicked(move |_| f(()));
        let f = self.with(|w, ()| w.apply());
        self.apply_button.connect_clicked(move |_| f(()));
        let f = self.with(|w, ()| identify::show(&w.outputs.borrow()));
        identify_button.connect_clicked(move |_| f(()));
        // Ctrl+Enter aplica, de qualquer campo
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let f = self.with(|w, ()| {
            if w.apply_button.is_sensitive() {
                w.apply();
            }
        });
        keys.connect_key_pressed(move |_, key, _, state| {
            if state.contains(gdk::ModifierType::CONTROL_MASK) && matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) {
                f(());
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.window.add_controller(keys);
    }

    // --- Estado ---

    /// Relê as saídas do Sway e descarta as mudanças
    fn reload(&self) {
        let Some(list) = sway::get_outputs() else {
            self.show_info(&tr("Could not read the screens from Sway. This app works only in a Sway session."));
            self.apply_button.set_sensitive(false);
            return;
        };
        let name = self.outputs.borrow().get(self.selected.get()).map(|o| o.name.clone());
        *self.edits.borrow_mut() = list.iter().map(Output::config).collect();
        // A mesma tela de antes; na primeira leitura, a que tem o foco (onde a pessoa está)
        let selected = name
            .and_then(|n| list.iter().position(|o| o.name == n))
            .or_else(|| list.iter().position(|o| o.focused))
            .unwrap_or(0);
        *self.outputs.borrow_mut() = list;
        self.selected.set(selected);
        self.fill_screens();
        self.refresh();
    }

    /// Alguma mudança ainda não aplicada?
    fn dirty(&self) -> bool {
        let outputs = self.outputs.borrow();
        self.edits.borrow().iter().zip(outputs.iter()).any(|(edit, output)| !edit.matches(&output.config()))
    }

    fn fill_screens(&self) {
        self.updating.set(true);
        let names: Vec<String> = self
            .outputs
            .borrow()
            .iter()
            .map(|o| {
                let description = o.description();
                if description.is_empty() {
                    o.name.clone()
                } else {
                    format!("{} — {description}", o.name)
                }
            })
            .collect();
        let model = gtk4::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
        self.screens.set_model(Some(&model));
        self.screens.set_selected(self.selected.get() as u32);
        self.updating.set(false);
    }

    fn select(&self, index: usize) {
        if index < self.outputs.borrow().len() {
            self.selected.set(index);
            self.refresh();
        }
    }

    /// Os campos, o mapa e os botões a partir do estado
    fn refresh(&self) {
        self.updating.set(true);
        let outputs = self.outputs.borrow();
        let edits = self.edits.borrow();
        let index = self.selected.get();
        if let (Some(output), Some(edit)) = (outputs.get(index), edits.get(index)) {
            set_selected(&self.screens, index);
            if self.enabled.is_active() != edit.enabled {
                self.enabled.set_active(edit.enabled);
            }
            let sizes = resolutions(output);
            let labels: Vec<String> = sizes.iter().map(|(w, h)| format!("{w} × {h}")).collect();
            set_choices(&self.resolution, &labels);
            let size = edit.mode.map(|m| (m.width, m.height));
            if let Some(i) = size.and_then(|s| sizes.iter().position(|x| *x == s)) {
                set_selected(&self.resolution, i);
            }
            let rate_modes = size.map(|s| rates(output, s)).unwrap_or_default();
            let labels: Vec<String> = rate_modes.iter().map(Mode::refresh_label).collect();
            set_choices(&self.rate, &labels);
            if let Some(i) = edit.mode.and_then(|m| rate_modes.iter().position(|x| (x.refresh - m.refresh).abs() <= 500)) {
                set_selected(&self.rate, i);
            }
            if (self.scale.value() - edit.scale).abs() > 1e-9 {
                self.scale.set_value(edit.scale);
            }
            let (w, h) = edit.logical_size();
            self.scale_info.set_text(&if w > 0 { fill_one(&tr("Effective size: %s"), &format!("{w} × {h}")) } else { String::new() });
            set_selected(&self.rotation, Transform::ALL.iter().position(|t| *t == edit.transform).unwrap_or(0));
            let has_modes = !output.modes.is_empty();
            self.resolution.set_sensitive(edit.enabled && has_modes);
            self.rate.set_sensitive(edit.enabled && has_modes);
            self.scale.set_sensitive(edit.enabled);
            self.rotation.set_sensitive(edit.enabled);
        }
        drop(edits);
        drop(outputs);
        let dirty = self.dirty();
        // Durante a prévia a pergunta é modal: os botões não mudam, e o foco volta a eles depois
        self.apply_button.set_sensitive(dirty);
        self.reset_button.set_sensitive(dirty);
        self.updating.set(false);
        self.area.queue_draw();
    }

    fn show_info(&self, text: &str) {
        self.info.set_text(text);
        self.info.set_visible(!text.is_empty());
    }

    /// Os retângulos lógicos das telas ligadas: (índice, retângulo)
    fn rects(&self) -> Vec<(usize, Rect)> {
        self.edits
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, e)| e.enabled)
            .map(|(i, e)| {
                let (w, h) = e.logical_size();
                (i, Rect { x: e.x, y: e.y, w: w.max(1), h: h.max(1) })
            })
            .collect()
    }

    /// Muda a tela escolhida e acerta as vizinhas: quem estava encostada à direita ou embaixo acompanha a
    /// mudança de tamanho; depois, nada sobreposto e o canto em 0,0
    fn change(&self, f: impl FnOnce(&mut OutputConfig)) {
        let index = self.selected.get();
        let before = self.rects().into_iter().find(|(i, _)| *i == index).map(|(_, r)| r);
        {
            let mut edits = self.edits.borrow_mut();
            let Some(edit) = edits.get_mut(index) else { return };
            f(edit);
        }
        let after = self.rects().into_iter().find(|(i, _)| *i == index).map(|(_, r)| r);
        if let (Some(before), Some(after)) = (before, after) {
            let (dw, dh) = (after.w - before.w, after.h - before.h);
            let mut edits = self.edits.borrow_mut();
            for (i, edit) in edits.iter_mut().enumerate() {
                if i == index || !edit.enabled {
                    continue;
                }
                if edit.x >= before.right() {
                    edit.x += dw;
                }
                if edit.y >= before.bottom() {
                    edit.y += dh;
                }
            }
        }
        self.settle(index);
        self.refresh();
    }

    /// Tira a tela `index` de cima das outras e põe o canto em 0,0
    fn settle(&self, index: usize) {
        let rects = self.rects();
        if let Some((_, moving)) = rects.iter().find(|(i, _)| *i == index) {
            let others: Vec<Rect> = rects.iter().filter(|(i, _)| *i != index).map(|(_, r)| *r).collect();
            let placed = geometry::unoverlap(*moving, &others);
            let mut edits = self.edits.borrow_mut();
            edits[index].x = placed.x;
            edits[index].y = placed.y;
        }
        let rects = self.rects();
        let mut plain: Vec<Rect> = rects.iter().map(|(_, r)| *r).collect();
        geometry::normalize(&mut plain);
        let mut edits = self.edits.borrow_mut();
        for ((i, _), r) in rects.iter().zip(plain) {
            edits[*i].x = r.x;
            edits[*i].y = r.y;
        }
    }

    fn set_enabled(&self, on: bool) {
        let index = self.selected.get();
        if !on && self.rects().iter().all(|(i, _)| *i == index) {
            self.show_info(&tr("At least one screen must stay on."));
            self.refresh();
            return;
        }
        self.show_info("");
        let others: Vec<Rect> = self.rects().into_iter().filter(|(i, _)| *i != index).map(|(_, r)| r).collect();
        let preferred = self.outputs.borrow().get(index).and_then(|o| o.modes.first().copied());
        self.change(|edit| {
            edit.enabled = on;
            if on {
                if edit.mode.is_none() {
                    edit.mode = preferred;
                }
                let place = geometry::place_new(edit.logical_size(), &others);
                edit.x = place.x;
                edit.y = place.y;
            }
        });
    }

    fn set_resolution(&self, choice: usize) {
        let mode = {
            let outputs = self.outputs.borrow();
            let Some(output) = outputs.get(self.selected.get()) else { return };
            let Some(size) = resolutions(output).get(choice).copied() else { return };
            // A maior taxa da resolução nova
            rates(output, size).first().copied()
        };
        if let Some(mode) = mode {
            self.change(|edit| edit.mode = Some(mode));
        }
    }

    fn set_rate(&self, choice: usize) {
        let mode = {
            let outputs = self.outputs.borrow();
            let Some(output) = outputs.get(self.selected.get()) else { return };
            let Some(current) = self.edits.borrow().get(self.selected.get()).and_then(|e| e.mode) else { return };
            rates(output, (current.width, current.height)).get(choice).copied()
        };
        if let Some(mode) = mode {
            self.change(|edit| edit.mode = Some(mode));
        }
    }

    fn set_scale(&self, value: f64) {
        // Em milésimos: o passo de 0,05 do campo soma erros de ponto flutuante (1.2500000000000002)
        let value = (value.clamp(SCALE_MIN, SCALE_MAX) * 1000.0).round() / 1000.0;
        self.change(|edit| edit.scale = value);
    }

    fn set_transform(&self, transform: Transform) {
        self.change(|edit| edit.transform = transform);
    }

    fn reset(&self) {
        self.show_info("");
        let current: Vec<OutputConfig> = self.outputs.borrow().iter().map(Output::config).collect();
        *self.edits.borrow_mut() = current;
        self.refresh();
    }

    // --- O mapa ---

    fn compute_view(&self, width: f64, height: f64) -> Option<View> {
        let rects = self.rects();
        let min_x = rects.iter().map(|(_, r)| r.x).min()? as f64;
        let min_y = rects.iter().map(|(_, r)| r.y).min()? as f64;
        let max_x = rects.iter().map(|(_, r)| r.right()).max()? as f64;
        let max_y = rects.iter().map(|(_, r)| r.bottom()).max()? as f64;
        let (bw, bh) = ((max_x - min_x).max(1.0), (max_y - min_y).max(1.0));
        let scale = ((width - 2.0 * MAP_MARGIN) / bw).min((height - 2.0 * MAP_MARGIN) / bh).max(0.01);
        Some(View { scale, ox: (width - bw * scale) / 2.0 - min_x * scale, oy: (height - bh * scale) / 2.0 - min_y * scale })
    }

    fn draw(&self, area: &gtk4::DrawingArea, cr: &gtk4::cairo::Context, width: f64, height: f64) {
        // Durante o arrasto, o mapa não muda de escala sob o mouse
        let view = match self.drag.borrow().as_ref() {
            Some(drag) => Some(drag.view),
            None => self.compute_view(width, height),
        };
        self.view.set(view);
        let Some(view) = view else { return };
        let fg = area.color();
        let outputs = self.outputs.borrow();
        for (i, rect) in self.rects() {
            let (x, y, w, h) = view.to_screen(&rect);
            let selected = i == self.selected.get();
            let fill = if selected { hex_rgba(&self.accent, 0.85) } else { gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.12) };
            cr.set_source_rgba(fill.red() as f64, fill.green() as f64, fill.blue() as f64, fill.alpha() as f64);
            cr.rectangle(x + 1.0, y + 1.0, w - 2.0, h - 2.0);
            let _ = cr.fill_preserve();
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.5);
            cr.set_line_width(1.0);
            let _ = cr.stroke();
            let Some(output) = outputs.get(i) else { continue };
            let text = format!("<b>{}</b>\n{}", glib::markup_escape_text(&output.name), glib::markup_escape_text(&output.description()));
            let layout = area.create_pango_layout(None);
            layout.set_markup(&text);
            layout.set_alignment(gtk4::pango::Alignment::Center);
            layout.set_width(((w - 8.0).max(1.0) * gtk4::pango::SCALE as f64) as i32);
            layout.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            let (_, lh) = layout.pixel_size();
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 1.0);
            cr.move_to(x + 4.0, y + (h - lh as f64) / 2.0);
            pangocairo::functions::show_layout(cr, &layout);
        }
    }

    fn hit(&self, x: f64, y: f64) -> Option<usize> {
        let view = self.view.get()?;
        self.rects().into_iter().rev().find_map(|(i, r)| {
            let (rx, ry, rw, rh) = view.to_screen(&r);
            (x >= rx && x <= rx + rw && y >= ry && y <= ry + rh).then_some(i)
        })
    }

    fn drag_begin(&self, x: f64, y: f64) {
        if self.preview.borrow().is_some() {
            return;
        }
        let Some(index) = self.hit(x, y) else { return };
        let Some(view) = self.view.get() else { return };
        self.selected.set(index);
        let start = self.edits.borrow().get(index).map(|e| (e.x, e.y)).unwrap_or_default();
        *self.drag.borrow_mut() = Some(Drag { index, start, view });
        self.refresh();
    }

    fn drag_update(&self, dx: f64, dy: f64) {
        let (index, start, view) = match self.drag.borrow().as_ref() {
            Some(d) => (d.index, d.start, d.view),
            None => return,
        };
        let rects = self.rects();
        let Some((_, rect)) = rects.iter().find(|(i, _)| *i == index) else { return };
        let moved = Rect { x: start.0 + (dx / view.scale).round() as i32, y: start.1 + (dy / view.scale).round() as i32, ..*rect };
        let others: Vec<Rect> = rects.iter().filter(|(i, _)| *i != index).map(|(_, r)| *r).collect();
        let snapped = geometry::snap(moved, &others, (SNAP_PIXELS / view.scale).round() as i32);
        {
            let mut edits = self.edits.borrow_mut();
            edits[index].x = snapped.x;
            edits[index].y = snapped.y;
        }
        self.area.queue_draw();
    }

    fn drag_end(&self) {
        let Some(drag) = self.drag.borrow_mut().take() else { return };
        self.settle(drag.index);
        self.refresh();
    }

    // --- Aplicar, manter ou desfazer ---

    fn named(&self, configs: &[OutputConfig]) -> Vec<(String, OutputConfig)> {
        self.outputs.borrow().iter().map(|o| o.name.clone()).zip(configs.iter().copied()).collect()
    }

    fn apply(self: &Rc<Self>) {
        if self.preview.borrow().is_some() {
            return;
        }
        // O serviço confere a trava pegando e soltando: se for neste instante, uma segunda tentativa basta
        let lock = take_preview_lock().or_else(|| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            take_preview_lock()
        });
        let Some(lock) = lock else {
            self.show_info(&tr("Another Displays window is waiting for a confirmation."));
            return;
        };
        *self.preview.borrow_mut() = Some(lock);
        // Trocar o modo pode levar alguns segundos, e a janela fica parada até a pergunta aparecer
        self.show_info(&tr("Applying…"));
        let previous: Vec<OutputConfig> = self.outputs.borrow().iter().map(Output::config).collect();
        let previous = self.named(&previous);
        let wanted = self.named(&self.edits.borrow());
        self.refresh();
        let me = self.me.clone();
        glib::MainContext::default().spawn_local(async move {
            let text = commands(&wanted);
            let states = switches(&wanted);
            let result = gio::spawn_blocking(move || (sway::focused_workspace(), sway::run_checked(&text, &states)))
                .await
                .map(|(home, result)| {
                    if let Some(w) = me.upgrade() {
                        *w.home.borrow_mut() = home;
                    }
                    result
                })
                .unwrap_or_else(|_| Err("?".into()));
            let Some(w) = me.upgrade() else { return };
            match result {
                Ok(()) => w.confirm(previous),
                Err(err) => {
                    warn(&format!("could not apply: {err}"));
                    w.restore(previous, Some(format!("{}: {err}", tr("Sway did not accept the change"))));
                }
            }
        });
    }

    /// Volta ao que era e solta a trava
    fn restore(self: &Rc<Self>, previous: Vec<(String, OutputConfig)>, message: Option<String>) {
        let me = self.me.clone();
        glib::MainContext::default().spawn_local(async move {
            let text = commands(&previous);
            let states = switches(&previous);
            let home = w_home(&me);
            // Se nem a volta pegou, a rede de segurança do serviço: o serviço ignora os eventos durante a
            // prévia, e nenhum outro evento viria para acioná-la
            let result = gio::spawn_blocking(move || {
                let result = sway::run_checked(&text, &states);
                match (&result, home) {
                    (Err(_), _) => crate::daemon::rescue_screens("undo"),
                    (Ok(()), Some((name, output))) => sway::return_workspace(&name, &output),
                    _ => {}
                }
                result
            })
            .await.unwrap_or_else(|_| Err("?".into()));
            let Some(w) = me.upgrade() else { return };
            if let Err(err) = &result {
                warn(&format!("could not restore: {err}"));
            }
            w.preview.borrow_mut().take();
            let edits = w.edits.borrow().clone();
            w.reload_keeping(edits);
            w.show_info(&message.unwrap_or_default());
        });
    }

    /// Relê as saídas e mantém as mudanças ainda não aplicadas (depois de desfazer, o usuário pode corrigir
    /// e tentar de novo)
    fn reload_keeping(&self, edits: Vec<OutputConfig>) {
        let names: Vec<String> = self.outputs.borrow().iter().map(|o| o.name.clone()).collect();
        self.reload();
        let same = self.outputs.borrow().iter().map(|o| o.name.clone()).collect::<Vec<_>>() == names;
        if same {
            *self.edits.borrow_mut() = edits;
            self.refresh();
        }
    }

    /// Guarda o perfil do conjunto e o arquivo do Sway
    fn keep(self: &Rc<Self>) {
        self.preview.borrow_mut().take();
        self.home.borrow_mut().take();
        let mut message = tr("Saved. This layout comes back whenever these screens are connected.");
        let outputs = self.outputs.borrow().clone();
        let edits = self.edits.borrow().clone();
        let keys = outputs::keys(&outputs);
        let file = profiles::path();
        let mut store = match Profiles::load(&file) {
            Ok(p) => p,
            Err(err) => {
                // Um arquivo estragado é guardado ao lado antes de ser trocado
                warn(&format!("replacing unreadable profiles: {err}"));
                let _ = std::fs::rename(&file, file.with_extension("json.bad"));
                Profiles::default()
            }
        };
        store.store(Profile { outputs: keys.iter().cloned().zip(edits.iter().copied()).collect() });
        if let Err(err) = store.save(&file) {
            message = format!("{}: {err}", tr("Could not save the layout"));
        }
        let items: Vec<(String, String, OutputConfig)> =
            outputs.iter().zip(keys).zip(edits).map(|((o, k), e)| (o.name.clone(), k, e)).collect();
        if let Err(err) = conf::save(&conf::path(), &conf::render(&items)) {
            message = format!("{}: {err}", tr("Could not save the layout"));
        }
        self.reload();
        self.show_info(&message);
    }

    /// A pergunta "Manter ou desfazer", que desfaz sozinha em 15 s
    fn confirm(self: &Rc<Self>, previous: Vec<(String, OutputConfig)>) {
        self.show_info("");
        let dialog = gtk4::Window::builder().transient_for(&self.window).modal(true).resizable(false).build();
        dialog.set_title(Some(&tr("Keep these display settings?")));
        let question = gtk4::Label::new(Some(&tr("Keep these display settings?")));
        question.add_css_class("title-3");
        let countdown = gtk4::Label::new(None);
        let undo = gtk4::Button::with_label(&tr("Undo"));
        let keep = gtk4::Button::with_label(&tr("Keep"));
        keep.add_css_class("suggested-action");
        let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        buttons.set_halign(gtk4::Align::End);
        buttons.append(&undo);
        buttons.append(&keep);
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        content.set_margin_top(20);
        content.set_margin_bottom(16);
        content.set_margin_start(20);
        content.set_margin_end(20);
        content.append(&question);
        content.append(&countdown);
        content.append(&buttons);
        dialog.set_child(Some(&content));

        // Quem responde primeiro (botão, Esc, fechar ou o tempo) decide; o resto é ignorado
        let answer: Rc<RefCell<Option<Box<dyn FnOnce(bool)>>>> = Rc::new(RefCell::new(None));
        let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        {
            let me = self.me.clone();
            let dialog = dialog.clone();
            let timer = timer.clone();
            *answer.borrow_mut() = Some(Box::new(move |kept: bool| {
                if let Some(id) = timer.borrow_mut().take() {
                    id.remove();
                }
                dialog.destroy();
                let Some(w) = me.upgrade() else { return };
                if kept {
                    w.keep();
                } else {
                    w.restore(previous, Some(tr("The previous settings are back.")));
                }
            }));
        }
        let decide = {
            let answer = answer.clone();
            move |kept: bool| {
                let f = answer.borrow_mut().take();
                if let Some(f) = f {
                    f(kept);
                }
            }
        };
        let left = Rc::new(Cell::new(CONFIRM_SECONDS));
        let set_text = {
            let countdown = countdown.clone();
            move |n: u32| countdown.set_text(&tr("Going back to the previous settings in %d s").replacen("%d", &n.to_string(), 1))
        };
        set_text(CONFIRM_SECONDS);
        {
            let decide = decide.clone();
            let timer_ref = timer.clone();
            let id = glib::timeout_add_seconds_local(1, move || {
                let n = left.get().saturating_sub(1);
                left.set(n);
                set_text(n);
                if n == 0 {
                    // O timer termina aqui: sai da lista antes de decidir, para não ser removido duas vezes
                    timer_ref.borrow_mut().take();
                    let decide = decide.clone();
                    glib::idle_add_local_once(move || decide(false));
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
            *timer.borrow_mut() = Some(id);
        }
        undo.connect_clicked({
            let decide = decide.clone();
            move |_| decide(false)
        });
        keep.connect_clicked({
            let decide = decide.clone();
            move |_| decide(true)
        });
        let keys = gtk4::EventControllerKey::new();
        keys.connect_key_pressed({
            let decide = decide.clone();
            move |_, key, _, _| {
                if key == gdk::Key::Escape {
                    decide(false);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        });
        dialog.add_controller(keys);
        dialog.connect_close_request({
            let decide = decide.clone();
            move |_| {
                decide(false);
                glib::Propagation::Stop
            }
        });
        dialog.set_default_widget(Some(&keep));
        dialog.present();
        undo.grab_focus();
    }

    // --- Monitores plugados com a janela aberta ---

    fn watch(self: &Rc<Self>) {
        let Ok((mut child, reader)) = sway::subscribe() else { return };
        let me = self.me.clone();
        glib::MainContext::default().spawn_local(async move {
            let mut reader = Some(reader);
            loop {
                let Some(mut lines) = reader.take() else { break };
                let Ok((lines, line)) = gio::spawn_blocking(move || {
                    let line = sway::next_line(&mut lines);
                    (lines, line)
                })
                .await
                else {
                    break;
                };
                let Some(line) = line else { break };
                let Some(w) = me.upgrade() else { break };
                if sway::event(&line).is_some() && w.preview.borrow().is_none() && w.drag.borrow().is_none() {
                    w.on_outputs_changed();
                }
                reader = Some(lines);
            }
            let _ = child.kill();
            let _ = child.wait();
        });
    }

    fn on_outputs_changed(&self) {
        let before: Vec<String> = profiles::set_of(&outputs::keys(&self.outputs.borrow()));
        if self.dirty() {
            let edits = self.edits.borrow().clone();
            let Some(list) = sway::get_outputs() else { return };
            let after = profiles::set_of(&outputs::keys(&list));
            if after == before {
                // Mesmo conjunto: as mudanças continuam, comparadas com o estado novo
                *self.outputs.borrow_mut() = list;
                *self.edits.borrow_mut() = edits;
                self.refresh();
                return;
            }
        }
        self.reload();
    }
}

/// O `%s` do texto traduzido trocado pelo valor
fn fill_one(template: &str, value: &str) -> String {
    template.replacen("%s", value, 1)
}
