//! Colors. Two palettes: the default and a color-blind-friendly one that
//! avoids red/green contrasts (blue-yellow ramps).

use ratatui::style::Color;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaletteKind {
    Classic,
    ColorBlind,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub kind: PaletteKind,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
    pub info: Color,
}

impl Palette {
    pub fn new(kind: PaletteKind) -> Palette {
        match kind {
            PaletteKind::Classic => Palette {
                kind,
                good: Color::Rgb(110, 220, 120),
                warn: Color::Rgb(240, 200, 60),
                bad: Color::Rgb(250, 80, 70),
                info: Color::Rgb(170, 190, 210),
            },
            PaletteKind::ColorBlind => Palette {
                kind,
                good: Color::Rgb(90, 170, 255),
                warn: Color::Rgb(250, 230, 90),
                bad: Color::Rgb(255, 140, 0),
                info: Color::Rgb(200, 200, 200),
            },
        }
    }

    /// A 0..1 ramp from bad to good (red->yellow->green, or orange->yellow->blue).
    pub fn ramp(self, v: f64) -> (u8, u8, u8) {
        let v = v.clamp(0.0, 1.0);
        let (lo, mid, hi) = match self.kind {
            PaletteKind::Classic => ((170, 40, 40), (170, 150, 40), (40, 140, 60)),
            PaletteKind::ColorBlind => ((190, 100, 10), (170, 160, 60), (30, 90, 190)),
        };
        if v < 0.5 {
            lerp(lo, mid, v * 2.0)
        } else {
            lerp(mid, hi, (v - 0.5) * 2.0)
        }
    }

    pub fn ramp_color(self, v: f64) -> Color {
        rgb(self.ramp(v))
    }
}

pub fn lerp(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

pub fn rgb(c: (u8, u8, u8)) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

pub fn scale(c: (u8, u8, u8), f: f64) -> (u8, u8, u8) {
    let g = |x: u8| ((x as f64) * f).clamp(0.0, 255.0) as u8;
    (g(c.0), g(c.1), g(c.2))
}

/// Distinct train colors.
pub const TRAIN_COLORS: [(u8, u8, u8); 8] = [
    (255, 210, 60),
    (80, 230, 255),
    (255, 120, 220),
    (160, 255, 120),
    (255, 160, 80),
    (190, 150, 255),
    (255, 255, 255),
    (120, 200, 160),
];
