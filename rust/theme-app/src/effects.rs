//! Os efeitos do SwayFX: cantos, blur, sombras, o blur das camadas, o escurecimento e as animações, do [effects] do
//! config.toml. A seção só aparece no SwayFX (desktop::swayfx, o mesmo `sway --version` que decide o 10-effects.conf);
//! no Sway puro ela nem é mostrada. Aplica ao mudar, sem o botão Aplicar, como o tamanho do cursor: as mudanças de uma
//! rajada (a SpinButton e a Scale mudam a cada passo) esperam EFFECT_DELAY_MS depois da última e vão juntas pelo
//! apply_effects (uma gravação, um reload), e a fila (effects::EffectQueue) não deixa duas aplicações correrem juntas.
//! As animações pedem o SwayFX 0.6: num 0.5, a linha some; com a versão ilegível, ela fica com o aviso.
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::config::{self, load_config, Effects, MAX_ANIMATION_DURATION_MS, MAX_CORNER_RADIUS};
use swaydesk_common::i18n::tr;
use swaydesk_theme::effects::EffectQueue;
use swaydesk_theme::themes as st;
use swaydesk_theme::{desktop as dc, fill_one};

/// A espera depois da última mudança, antes de aplicar
const EFFECT_DELAY_MS: u64 = 300;
/// A primeira versão do SwayFX com o animation_duration_ms
const ANIMATIONS_SINCE: (u32, u32) = (0, 6);

pub struct EffectsSection {
    /// O título, as linhas e a nota, tudo escondido até a detecção achar o SwayFX
    pub root: gtk4::Box,
    note: gtk4::Label,
    corner_radius: gtk4::SpinButton,
    blur: gtk4::Switch,
    shadows: gtk4::Switch,
    layer_blur: gtk4::Switch,
    dim_inactive: gtk4::Scale,
    animations_row: gtk4::Box,
    animation_duration_ms: gtk4::SpinButton,
    animations_hint: gtk4::Label,
    queue: RefCell<EffectQueue>,
    timer: RefCell<Option<glib::SourceId>>,
    /// Enchendo os controles (o load): a mudança não é do usuário
    updating: Cell<bool>,
}

/// Uma linha: o rótulo à esquerda, ligado ao controle pelo mnemonic (que também é o nome dele no leitor de tela), e o
/// controle à direita.
fn row(text: &str, control: &impl IsA<gtk4::Widget>) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_mnemonic_widget(Some(control));
    row.append(&label);
    row.append(control);
    row
}

fn switch() -> gtk4::Switch {
    let switch = gtk4::Switch::new();
    switch.set_valign(gtk4::Align::Center);
    switch
}

impl EffectsSection {
    pub fn new() -> Rc<EffectsSection> {
        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        root.set_margin_top(8);
        root.set_visible(false);
        let title = gtk4::Label::new(Some(&tr("SwayFX effects")));
        title.set_xalign(0.0);
        title.add_css_class("theme-section");
        root.append(&title);
        let corner_radius = gtk4::SpinButton::with_range(0.0, MAX_CORNER_RADIUS as f64, 1.0);
        let blur = switch();
        let shadows = switch();
        let layer_blur = switch();
        let dim_inactive = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 1.0, 0.05);
        dim_inactive.set_size_request(200, -1);
        dim_inactive.set_digits(2);
        dim_inactive.set_draw_value(true);
        let animation_duration_ms = gtk4::SpinButton::with_range(0.0, MAX_ANIMATION_DURATION_MS as f64, 50.0);
        root.append(&row(&tr("Rounded corners (px)"), &corner_radius));
        root.append(&row(&tr("Blur behind windows"), &blur));
        root.append(&row(&tr("Window shadows"), &shadows));
        root.append(&row(&tr("Blur behind the bar, dock and popups"), &layer_blur));
        root.append(&row(&tr("Dim inactive windows"), &dim_inactive));
        let animations_row = row(&tr("Animations (ms, 0 turns them off)"), &animation_duration_ms);
        root.append(&animations_row);
        let animations_hint = gtk4::Label::new(Some(&tr("Animations need SwayFX 0.6 or newer")));
        animations_hint.set_xalign(0.0);
        animations_hint.add_css_class("dim-label");
        animations_hint.set_visible(false);
        root.append(&animations_hint);
        let note = gtk4::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_visible(false);
        note.add_css_class("dim-label");
        root.append(&note);
        let this = Rc::new(EffectsSection {
            root,
            note,
            corner_radius,
            blur,
            shadows,
            layer_blur,
            dim_inactive,
            animations_row,
            animation_duration_ms,
            animations_hint,
            queue: RefCell::new(EffectQueue::default()),
            timer: RefCell::new(None),
            updating: Cell::new(false),
        });
        this.connect();
        this.detect();
        this
    }

    fn connect(self: &Rc<Self>) {
        let changed = |me: &Rc<Self>, key: &'static str, text: String| {
            if !me.updating.get() {
                me.changed(key, &text);
            }
        };
        let weak = Rc::downgrade(self);
        self.corner_radius.connect_value_changed(move |spin| {
            if let Some(me) = weak.upgrade() {
                changed(&me, "corner_radius", spin.value_as_int().to_string());
            }
        });
        for (switch, key) in [(&self.blur, "blur"), (&self.shadows, "shadows"), (&self.layer_blur, "layer_blur")] {
            let weak = Rc::downgrade(self);
            switch.connect_active_notify(move |switch| {
                if let Some(me) = weak.upgrade() {
                    changed(&me, key, switch.is_active().to_string());
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.dim_inactive.connect_value_changed(move |scale| {
            if let Some(me) = weak.upgrade() {
                // Em centésimos, como no config: o passo de 0.05 da Scale não vira 0.15000000000000002
                let hundredths = (scale.value() * 100.0).round() as u32;
                changed(&me, "dim_inactive", format!("{:?}", f64::from(hundredths) / 100.0));
            }
        });
        let weak = Rc::downgrade(self);
        self.animation_duration_ms.connect_value_changed(move |spin| {
            if let Some(me) = weak.upgrade() {
                changed(&me, "animation_duration_ms", spin.value_as_int().to_string());
            }
        });
    }

    /// O `sway --version` fora da thread da interface (até 5 s); a seção aparece só no SwayFX.
    fn detect(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let Ok(found) = gio::spawn_blocking(dc::swayfx).await else { return };
            let Some(me) = me.upgrade() else { return };
            let Some(version) = found else { return };
            // Num SwayFX que se sabe anterior ao 0.6, a diretiva daria erro de config: a linha some. Ilegível, fica com
            // o aviso
            let old = version.is_some_and(|v| v < ANIMATIONS_SINCE);
            me.animations_row.set_visible(!old);
            me.animations_hint.set_visible(version.is_none());
            me.root.set_visible(true);
        });
    }

    /// Os controles com o [effects] do config. Com mudanças esperando ou aplicando, nada: desfaria o que o usuário
    /// mexeu, e o fim da aplicação recarrega.
    pub fn load(&self, effects: &Effects) {
        if self.queue.borrow().busy() {
            return;
        }
        self.updating.set(true);
        self.corner_radius.set_value(f64::from(effects.corner_radius));
        self.blur.set_active(effects.blur);
        self.shadows.set_active(effects.shadows);
        self.layer_blur.set_active(effects.layer_blur);
        self.dim_inactive.set_value(f64::from(effects.dim_inactive) / 100.0);
        self.animation_duration_ms.set_value(f64::from(effects.animation_duration_ms));
        self.updating.set(false);
    }

    /// Uma mudança: entra na fila, e a espera recomeça.
    fn changed(self: &Rc<Self>, key: &str, text: &str) {
        self.queue.borrow_mut().push(key, text);
        self.schedule();
    }

    fn schedule(self: &Rc<Self>) {
        if let Some(old) = self.timer.borrow_mut().take() {
            old.remove();
        }
        let me = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(EFFECT_DELAY_MS), move || {
            if let Some(me) = me.upgrade() {
                me.timer.borrow_mut().take();
                me.run();
            }
        });
        *self.timer.borrow_mut() = Some(id);
    }

    /// Aplica o que esperava, se nenhuma aplicação está rodando (o fim dela chama de novo).
    fn run(self: &Rc<Self>) {
        let Some(changes) = self.queue.borrow_mut().start() else { return };
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || st::locked(|| st::apply_effects(&changes)).and_then(|r| r)).await;
            let Some(me) = me.upgrade() else { return };
            let text = match result {
                Ok(Ok(applied)) => applied.notes.join("\n"),
                Ok(Err(err)) => err.to_string(),
                Err(_) => fill_one(&tr("Could not update: %s"), &tr("SwayFX effects")),
            };
            me.note.set_text(&text);
            me.note.set_visible(!text.is_empty());
            if me.queue.borrow_mut().finish() {
                me.run();
                return;
            }
            // Os controles voltam ao config: o aplicado ou, se falhou, o de antes. Um config que não se lê daria o
            // padrão, que não é o de ninguém: aí ficam como estão
            let (config, problems) = load_config(&config::path());
            if !problems.iter().any(|p| p.contains("could not read")) {
                me.load(&config.effects);
            }
        });
    }
}
