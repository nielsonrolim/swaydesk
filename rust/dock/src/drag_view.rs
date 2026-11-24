//! A fileira de itens da dock, com a animação do arrasto: o item arrastado levanta (maior, com sombra e um
//! pouco transparente) e segue o ponteiro, os vizinhos deslizam para abrir espaço, e o solto assenta no
//! lugar dele. Tudo só no desenho (snapshot): o lugar de cada item, os cliques e a conta do arrasto
//! (dl::drag_index, as larguras) são os do layout, sem transformação nenhuma.
//!
//! O deslize dos vizinhos é o FLIP: quando um item muda de lugar, o desenho começa no lugar antigo e volta
//! ao novo. Um timer do GLib anima e para assim que tudo está em repouso; sem animação (a dock decide),
//! nada aqui é chamado e a fileira é um Box comum.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{glib, graphene};
use swaydesk_dock_logic as dl;

const FRAME: Duration = Duration::from_millis(16);

fn now_ms() -> i64 {
    glib::monotonic_time() / 1000
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct DragView {
        /// O item arrastado, do começo do arrasto até assentar
        pub lifted: RefCell<Option<gtk4::Widget>>,
        /// O ponto do item onde o arrasto o pegou
        pub grab: Cell<f64>,
        /// O x do ponteiro na fileira, enquanto o botão está apertado
        pub pointer: Cell<Option<f64>>,
        pub lift: RefCell<dl::Slide>,
        pub glides: RefCell<HashMap<gtk4::Widget, dl::Glide>>,
        /// A borda esquerda de cada item no último layout
        pub bases: RefCell<HashMap<gtk4::Widget, f64>>,
        pub timer: RefCell<Option<glib::SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DragView {
        const NAME: &'static str = "SwaydeskDockDragView";
        type Type = super::DragView;
        type ParentType = gtk4::Box;
    }

    impl ObjectImpl for DragView {}

    impl WidgetImpl for DragView {
        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            self.sync_layout();
            let obj = self.obj();
            let lifted = self.lifted.borrow().clone();
            // O levantado por último: por cima dos vizinhos
            for child in super::children(&*obj) {
                if lifted.as_ref() != Some(&child) {
                    self.draw_child(&child, snapshot, false);
                }
            }
            if let Some(child) = lifted.filter(|c| c.parent().as_ref() == Some(obj.upcast_ref())) {
                self.draw_child(&child, snapshot, true);
            }
        }
    }

    impl BoxImpl for DragView {}

    impl DragView {
        /// O lugar de cada item no layout atual, e o FLIP de quem mudou de lugar desde a última vez. No
        /// snapshot, e não num size_allocate: o do Box é o do layout manager, e um override não roda.
        pub fn sync_layout(&self) {
            let obj = self.obj();
            let now = now_ms();
            let bases: HashMap<gtk4::Widget, f64> = super::children(&*obj)
                .into_iter()
                .filter_map(|child| child.compute_bounds(&*obj).map(|r| (child, r.x() as f64)))
                .collect();
            // O arrastado saiu da fileira (os apps instalados mudaram ou o fixado saiu enquanto assentava):
            // o resto do assentar não tem o que desenhar
            let gone = self.lifted.borrow().as_ref().is_some_and(|c| !bases.contains_key(c));
            if gone {
                self.lifted.borrow_mut().take();
                self.pointer.set(None);
            }
            let old = self.bases.replace(bases);
            if self.lifted.borrow().is_none() {
                return; // fora do arrasto, nada desliza: um app que abre ou fecha muda a fileira como sempre
            }
            let mut moved = false;
            {
                let held = self.pointer.get().and(self.lifted.borrow().clone());
                let mut glides = self.glides.borrow_mut();
                let bases = self.bases.borrow();
                glides.retain(|child, _| bases.contains_key(child));
                for (child, base) in bases.iter() {
                    // O arrastado segue o ponteiro, não o lugar
                    let Some(before) = old.get(child) else { continue };
                    if held.as_ref() == Some(child) || before == base {
                        continue;
                    }
                    glides.entry(child.clone()).or_default().moved(before - base, now);
                    moved = true;
                }
            }
            if moved {
                obj.ensure_timer();
            }
        }

        pub fn shift(&self, child: &gtk4::Widget, now: i64) -> f64 {
            let held = self.pointer.get().filter(|_| self.lifted.borrow().as_ref() == Some(child));
            match held {
                Some(pointer) => {
                    let obj = self.obj();
                    let base = self.bases.borrow().get(child).copied().unwrap_or(0.0);
                    // A borda do item sem a margem do CSS (no GTK 3 a alocação a inclui): nas pontas, o
                    // arrastado pode passar 4 px do lugar dele, dentro da margem
                    let width = child.compute_bounds(&*obj).map_or(0.0, |r| r.width() as f64);
                    dl::held_shift(pointer, self.grab.get(), base, width, obj.width() as f64)
                }
                None => self.glides.borrow().get(child).map_or(0.0, |g| g.value(now)),
            }
        }

        fn draw_child(&self, child: &gtk4::Widget, snapshot: &gtk4::Snapshot, lifted: bool) {
            let obj = self.obj();
            let shift = self.shift(child, now_ms());
            let progress = if lifted { self.lift.borrow().progress } else { 0.0 };
            if shift == 0.0 && progress == 0.0 {
                obj.snapshot_child(child, snapshot);
                return;
            }
            snapshot.save();
            snapshot.translate(&graphene::Point::new(shift as f32, 0.0));
            if progress > 0.0 {
                if let Some(bounds) = child.compute_bounds(&*obj) {
                    let center = bounds.center();
                    let scale = dl::lift_scale(progress) as f32;
                    snapshot.translate(&center);
                    snapshot.scale(scale, scale);
                    snapshot.translate(&graphene::Point::new(-center.x(), -center.y()));
                }
                snapshot.push_opacity(dl::lift_opacity(progress));
                obj.snapshot_child(child, snapshot);
                snapshot.pop();
            } else {
                obj.snapshot_child(child, snapshot);
            }
            snapshot.restore();
        }
    }
}

glib::wrapper! {
    pub struct DragView(ObjectSubclass<imp::DragView>)
        @extends gtk4::Box, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Orientable;
}

impl Default for DragView {
    fn default() -> Self {
        glib::Object::builder().property("orientation", gtk4::Orientation::Horizontal).build()
    }
}

impl DragView {
    /// O arrasto de `child` começou, pego no ponto `grab` do item: ele levanta.
    pub fn lift(&self, child: &gtk4::Widget, grab: f64) {
        let imp = self.imp();
        if let Some(previous) = imp.lifted.replace(Some(child.clone())) {
            previous.remove_css_class("dragging");
        }
        imp.grab.set(grab);
        imp.pointer.set(None);
        imp.glides.borrow_mut().remove(child);
        imp.sync_layout();
        imp.lift.borrow_mut().start(1.0, now_ms());
        child.add_css_class("dragging");
        self.ensure_timer();
    }

    /// O ponteiro está em x na fileira: o item levantado o segue.
    pub fn follow(&self, x: f64) {
        if self.imp().lifted.borrow().is_some() {
            self.imp().pointer.set(Some(x));
            self.queue_draw();
        }
    }

    /// O botão foi solto (ou o arrasto acabou de outro jeito): o item assenta no lugar dele e desce.
    pub fn release(&self) {
        let imp = self.imp();
        let Some(child) = imp.lifted.borrow().clone() else { return };
        imp.sync_layout();
        let now = now_ms();
        let from = imp.shift(&child, now);
        imp.pointer.set(None);
        imp.glides.borrow_mut().insert(child.clone(), dl::Glide::new(from, now));
        imp.lift.borrow_mut().start(0.0, now);
        child.remove_css_class("dragging");
        self.ensure_timer();
    }

    /// Quantos itens estão fora do lugar agora e se o timer está rodando: para o dump (kill -USR2).
    pub fn state(&self) -> (usize, bool) {
        let imp = self.imp();
        imp.sync_layout();
        let now = now_ms();
        let shifted = children(self).iter().filter(|c| imp.shift(c, now) != 0.0).count();
        (shifted, imp.timer.borrow().is_some())
    }

    fn ensure_timer(&self) {
        let imp = self.imp();
        if imp.timer.borrow().is_some() {
            return;
        }
        let weak = self.downgrade();
        let timer = glib::timeout_add_local(FRAME, move || {
            let Some(view) = weak.upgrade() else { return glib::ControlFlow::Break };
            let running = view.step(now_ms());
            view.queue_draw();
            if running {
                return glib::ControlFlow::Continue;
            }
            view.imp().timer.borrow_mut().take();
            glib::ControlFlow::Break
        });
        *imp.timer.borrow_mut() = Some(timer);
    }

    /// Avança as animações até now. Devolve se alguma continua; em repouso e solto, o item arrastado volta
    /// a ser um item comum.
    fn step(&self, now: i64) -> bool {
        let imp = self.imp();
        let lifting = imp.lift.borrow_mut().step(now, dl::DRAG_ANIMATION_MS);
        let mut glides = imp.glides.borrow_mut();
        glides.retain(|_, g| g.running(now));
        let running = lifting || !glides.is_empty();
        if !running && imp.pointer.get().is_none() {
            imp.lifted.borrow_mut().take();
        }
        running
    }
}

fn children(container: &impl IsA<gtk4::Widget>) -> Vec<gtk4::Widget> {
    let mut found = Vec::new();
    let mut child = container.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        found.push(widget);
    }
    found
}
