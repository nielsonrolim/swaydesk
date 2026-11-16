//! A geometria do arrastar, em pixels lógicos: o encaixe nas bordas das outras telas, a saída de cima de
//! outra tela (o Sway aceita telas sobrepostas, mas o mouse fica preso) e o deslocamento que põe a menor
//! coordenada em 0,0.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// A distância (pixels lógicos) em que uma borda gruda na outra
pub const SNAP: i32 = 24;

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.right() && other.x < self.right() && self.y < other.bottom() && other.y < self.bottom()
    }

    /// Encosta (borda com borda, com algum trecho em comum ou no canto)
    pub fn touches(&self, other: &Rect) -> bool {
        let side = (self.right() == other.x || other.right() == self.x) && self.y <= other.bottom() && other.y <= self.bottom();
        let top = (self.bottom() == other.y || other.bottom() == self.y) && self.x <= other.right() && other.x <= self.right();
        side || top
    }
}

/// O deslocamento mais curto (até `limit`) que alinha um dos dois lados do segmento [start, end) a uma das
/// bordas dadas
fn nearest(start: i32, end: i32, edges: &[i32], limit: i32) -> Option<i32> {
    let mut best: Option<i32> = None;
    for &edge in edges {
        for delta in [edge - start, edge - end] {
            if delta.abs() <= limit && best.is_none_or(|b| delta.abs() < b.abs()) {
                best = Some(delta);
            }
        }
    }
    best
}

/// A posição de `moving` depois do encaixe nas bordas das outras: cada eixo gruda na borda mais próxima a
/// até [`SNAP`]. O limite é em pixels lógicos; na tela, o app passa o que corresponde a ~12 pixels do mapa.
pub fn snap(moving: Rect, others: &[Rect], limit: i32) -> Rect {
    let xs: Vec<i32> = others.iter().flat_map(|o| [o.x, o.right()]).collect();
    let ys: Vec<i32> = others.iter().flat_map(|o| [o.y, o.bottom()]).collect();
    let mut out = moving;
    if let Some(dx) = nearest(moving.x, moving.right(), &xs, limit) {
        out.x += dx;
    }
    if let Some(dy) = nearest(moving.y, moving.bottom(), &ys, limit) {
        out.y += dy;
    }
    out
}

/// Tira `moving` de cima das outras: entre as posições encostadas em cada lado de cada tela, a mais perto
/// da pedida que não sobrepõe nenhuma
pub fn unoverlap(moving: Rect, others: &[Rect]) -> Rect {
    if !others.iter().any(|o| o.overlaps(&moving)) {
        return moving;
    }
    let mut candidates = Vec::new();
    for o in others {
        candidates.push(Rect { x: o.right(), ..moving });
        candidates.push(Rect { x: o.x - moving.w, ..moving });
        candidates.push(Rect { y: o.bottom(), ..moving });
        candidates.push(Rect { y: o.y - moving.h, ..moving });
        // Nos cantos, quando nenhum lado sozinho serve
        candidates.push(Rect { x: o.right(), y: o.y, ..moving });
        candidates.push(Rect { x: o.x - moving.w, y: o.y, ..moving });
        candidates.push(Rect { x: o.x, y: o.bottom(), ..moving });
        candidates.push(Rect { x: o.x, y: o.y - moving.h, ..moving });
    }
    let distance = |r: &Rect| ((r.x - moving.x) as i64).pow(2) + ((r.y - moving.y) as i64).pow(2);
    candidates
        .into_iter()
        .filter(|c| !others.iter().any(|o| o.overlaps(c)))
        .min_by_key(distance)
        .unwrap_or(moving)
}

/// Põe a menor coordenada em 0,0 (o Sway aceita negativas, mas o Xwayland e alguns apps não gostam)
pub fn normalize(rects: &mut [Rect]) {
    let (Some(min_x), Some(min_y)) = (rects.iter().map(|r| r.x).min(), rects.iter().map(|r| r.y).min()) else { return };
    for r in rects.iter_mut() {
        r.x -= min_x;
        r.y -= min_y;
    }
}

/// Uma posição para a tela que acabou de ser ligada: à direita da mais à direita, no topo dela
pub fn place_new(size: (i32, i32), others: &[Rect]) -> Rect {
    let x = others.iter().map(Rect::right).max().unwrap_or(0);
    let y = others.iter().filter(|o| o.right() == x).map(|o| o.y).min().unwrap_or(0);
    Rect { x, y, w: size.0, h: size.1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPTOP: Rect = Rect { x: 0, y: 0, w: 1536, h: 864 };

    #[test]
    fn snaps_to_the_nearest_edge() {
        let moving = Rect { x: 1550, y: 10, w: 1920, h: 1080 };
        assert_eq!(snap(moving, &[LAPTOP], SNAP), Rect { x: 1536, y: 0, w: 1920, h: 1080 });
        // Longe demais: fica onde está
        let far = Rect { x: 1600, y: 100, w: 1920, h: 1080 };
        assert_eq!(snap(far, &[LAPTOP], SNAP), far);
        // A borda de baixo da que move gruda na de baixo da outra
        let below = Rect { x: 1536, y: -200, w: 1920, h: 1080 };
        assert_eq!(snap(below, &[LAPTOP], SNAP).bottom(), 864);
    }

    #[test]
    fn leaves_other_screens() {
        let moving = Rect { x: 1000, y: 100, w: 1920, h: 1080 };
        let out = unoverlap(moving, &[LAPTOP]);
        assert!(!out.overlaps(&LAPTOP));
        assert_eq!(out, Rect { x: 1536, y: 100, w: 1920, h: 1080 });
        let above = unoverlap(Rect { x: 0, y: -900, w: 1536, h: 1000 }, &[LAPTOP]);
        assert_eq!(above.bottom(), 0);
        assert!(above.touches(&LAPTOP));
    }

    #[test]
    fn normalizes_and_places() {
        let mut rects = [Rect { x: -1920, y: -100, w: 1920, h: 1080 }, LAPTOP];
        normalize(&mut rects);
        assert_eq!(rects[0], Rect { x: 0, y: 0, w: 1920, h: 1080 });
        assert_eq!(rects[1], Rect { x: 1920, y: 100, w: 1536, h: 864 });
        assert_eq!(place_new((800, 600), &rects), Rect { x: 3456, y: 100, w: 800, h: 600 });
        assert_eq!(place_new((800, 600), &[]), Rect { x: 0, y: 0, w: 800, h: 600 });
    }
}
