//! Turning characters into pixels for the game's window. Letters and symbols
//! come from the bundled DejaVu fonts (`assets/fonts`). Box-drawing and block
//! characters are drawn by hand instead, the way terminals do it, so track,
//! borders and bars join up seamlessly from one cell to the next.

use std::collections::HashMap;

use fontdue::{Font, FontSettings};

static MONO: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf");
static MONO_BOLD: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono-Bold.ttf");
/// For symbols the monospace face lacks (it is squeezed into the cell).
static FALLBACK: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");

pub struct Glyphs {
    fonts: [Font; 3],
    px: f32,
    /// Cell width and height in pixels.
    pub cw: usize,
    pub ch: usize,
    baseline: f32,
    /// Coverage maps (0..=255, `cw * cols` by `ch`), by character, boldness and width in cells.
    cache: HashMap<(char, bool, usize), Vec<u8>>,
}

impl Glyphs {
    pub fn new(px: f32) -> Glyphs {
        let load = |b: &[u8]| Font::from_bytes(b, FontSettings::default()).expect("bundled font");
        let mut g = Glyphs {
            fonts: [load(MONO), load(MONO_BOLD), load(FALLBACK)],
            px: 0.0,
            cw: 1,
            ch: 1,
            baseline: 0.0,
            cache: HashMap::new(),
        };
        g.set_px(px);
        g
    }

    /// Change the font size (pixels per em). Cells follow the monospace face.
    pub fn set_px(&mut self, px: f32) {
        if px == self.px {
            return;
        }
        (self.cw, self.ch) = self.cell_size(px);
        let lm = self.fonts[0]
            .horizontal_line_metrics(px)
            .expect("monospace font has line metrics");
        self.baseline = lm.ascent.round();
        self.px = px;
        self.cache.clear();
    }

    /// How big a cell would be at `px` (pixels per em), without switching to it.
    pub fn cell_size(&self, px: f32) -> (usize, usize) {
        let m = self.fonts[0].metrics('M', px);
        let lm = self.fonts[0]
            .horizontal_line_metrics(px)
            .expect("monospace font has line metrics");
        (
            m.advance_width.round().max(2.0) as usize,
            (lm.ascent - lm.descent).ceil().max(4.0) as usize,
        )
    }

    /// The coverage map for `c` drawn across `cols` cells.
    pub fn get(&mut self, c: char, bold: bool, cols: usize) -> &[u8] {
        let key = (c, bold, cols);
        if !self.cache.contains_key(&key) {
            let cov = self.render(c, bold, cols);
            self.cache.insert(key, cov);
        }
        &self.cache[&key]
    }

    /// Where an underline goes (a row of the cell).
    pub fn underline_row(&self) -> usize {
        (self.baseline as usize + 1 + self.ch / 16).min(self.ch - 1)
    }

    /// The coverage map for `c` stretched over a block `cols` by `rows` cells
    /// (a zoomed-in map tile): lines run through its middle to its edges, so
    /// they still join the blocks beside it, and letters grow with its height.
    pub fn get_block(&mut self, c: char, bold: bool, cols: usize, rows: usize) -> &[u8] {
        let key = (c, bold, cols | rows << 8);
        if !self.cache.contains_key(&key) {
            let (w, h) = (self.cw * cols, self.ch * rows);
            let mut cov = vec![0u8; w * h];
            if !(c == ' '
                || box_drawing(c, w, h, w, &mut cov)
                || block(c, w, h, w, &mut cov)
                || symbol(c, w, h, w, &mut cov))
            {
                self.font_glyph(
                    c,
                    bold,
                    self.px * rows as f32,
                    self.baseline * rows as f32,
                    w,
                    h,
                    &mut cov,
                );
            }
            self.cache.insert(key, cov);
        }
        &self.cache[&key]
    }

    fn render(&self, c: char, bold: bool, cols: usize) -> Vec<u8> {
        let (w, h) = (self.cw * cols, self.ch);
        let mut cov = vec![0u8; w * h];
        if c == ' '
            || box_drawing(c, self.cw, h, w, &mut cov)
            || block(c, self.cw, h, w, &mut cov)
            || symbol(c, self.cw, h, w, &mut cov)
        {
            return cov;
        }
        self.font_glyph(c, bold, self.px, self.baseline, w, h, &mut cov);
        cov
    }

    /// A letter or symbol from the fonts at `px`, centered across `w` (squeezed to fit).
    #[allow(clippy::too_many_arguments)]
    fn font_glyph(
        &self,
        c: char,
        bold: bool,
        px: f32,
        baseline: f32,
        w: usize,
        h: usize,
        cov: &mut [u8],
    ) {
        let has = |f: &Font| f.lookup_glyph_index(c) != 0;
        let mono = &self.fonts[if bold { 1 } else { 0 }];
        let (font, c) = if has(mono) {
            (mono, c)
        } else if has(&self.fonts[2]) {
            (&self.fonts[2], c)
        } else {
            (&self.fonts[0], '?')
        };
        let mut px = px;
        let m = font.metrics(c, px);
        // squeeze anything too wide for its cells (fallback symbols are often wide)
        if m.advance_width > w as f32 + 0.5 {
            px *= w as f32 / m.advance_width;
        }
        let (m, bmp) = font.rasterize(c, px);
        let ox = ((w as f32 - m.advance_width) / 2.0).round() as i32 + m.xmin;
        let oy = (baseline - m.ymin as f32 - m.height as f32).round() as i32;
        for by in 0..m.height {
            let y = oy + by as i32;
            if y < 0 || y >= h as i32 {
                continue;
            }
            for bx in 0..m.width {
                let x = ox + bx as i32;
                if x < 0 || x >= w as i32 {
                    continue;
                }
                let a = bmp[by * m.width + bx];
                let o = &mut cov[y as usize * w + x as usize];
                *o = (*o).max(a);
            }
        }
    }
}

/// Fill a rectangle, clipped to the cell: at tiny text sizes the line
/// layouts (double track especially) can reach past its edge.
fn fill(cov: &mut [u8], stride: usize, x0: usize, x1: usize, y0: usize, y1: usize, a: u8) {
    let rows = cov.len() / stride.max(1);
    for y in y0..y1.min(rows) {
        for x in x0..x1.min(stride) {
            cov[y * stride + x] = a;
        }
    }
}

const U: u8 = 1;
const D: u8 = 2;
const L: u8 = 4;
const R: u8 = 8;

/// Light, heavy and double box-drawing lines. Returns false for anything else.
fn box_drawing(c: char, cw: usize, ch: usize, stride: usize, cov: &mut [u8]) -> bool {
    let (arms, double) = match c {
        '─' => (L | R, false),
        '│' => (U | D, false),
        '┌' | '╭' => (R | D, false),
        '┐' | '╮' => (L | D, false),
        '└' | '╰' => (U | R, false),
        '┘' | '╯' => (U | L, false),
        '├' => (U | D | R, false),
        '┤' => (U | D | L, false),
        '┬' => (L | R | D, false),
        '┴' => (L | R | U, false),
        '┼' => (U | D | L | R, false),
        '═' => (L | R, true),
        '║' => (U | D, true),
        '╔' => (R | D, true),
        '╗' => (L | D, true),
        '╚' => (U | R, true),
        '╝' => (U | L, true),
        '╠' => (U | D | R, true),
        '╣' => (U | D | L, true),
        '╦' => (L | R | D, true),
        '╩' => (L | R | U, true),
        '╬' => (U | D | L | R, true),
        '━' | '┃' | '┏' | '┓' | '┗' | '┛' => return heavy(c, cw, ch, stride, cov),
        _ => return false,
    };
    let t = ((cw as f32 / 8.0).round() as usize).max(1);
    let has = |a: u8| arms & a != 0;
    if !double {
        single(arms, t, cw, ch, stride, cov);
        return true;
    }
    // Two lines each way: left/right verticals at xl/xr, top/bottom horizontals
    // at yt/yb. Where arms meet, the inner lines break and the outer ones turn.
    let gap = t.max(cw / 6).max(1);
    let xl = (cw.saturating_sub(2 * t + gap)) / 2;
    let xr = xl + t + gap;
    let yt = (ch.saturating_sub(2 * t + gap)) / 2;
    let yb = yt + t + gap;
    let (mx, my) = (cw / 2, ch / 2);
    if has(U) {
        let left_end = if has(L) {
            yt + t
        } else if has(R) {
            yb + t
        } else {
            my
        };
        let right_end = if has(R) {
            yt + t
        } else if has(L) {
            yb + t
        } else {
            my
        };
        fill(cov, stride, xl, xl + t, 0, left_end, 255);
        fill(cov, stride, xr, xr + t, 0, right_end, 255);
    }
    if has(D) {
        let left_start = if has(L) {
            yb
        } else if has(R) {
            yt
        } else {
            my
        };
        let right_start = if has(R) {
            yb
        } else if has(L) {
            yt
        } else {
            my
        };
        fill(cov, stride, xl, xl + t, left_start, ch, 255);
        fill(cov, stride, xr, xr + t, right_start, ch, 255);
    }
    if has(L) {
        let top_end = if has(U) {
            xl + t
        } else if has(D) {
            xr + t
        } else {
            mx
        };
        let bottom_end = if has(D) {
            xl + t
        } else if has(U) {
            xr + t
        } else {
            mx
        };
        fill(cov, stride, 0, top_end, yt, yt + t, 255);
        fill(cov, stride, 0, bottom_end, yb, yb + t, 255);
    }
    if has(R) {
        let top_start = if has(U) {
            xr
        } else if has(D) {
            xl
        } else {
            mx
        };
        let bottom_start = if has(D) {
            xr
        } else if has(U) {
            xl
        } else {
            mx
        };
        fill(cov, stride, top_start, cw, yt, yt + t, 255);
        fill(cov, stride, bottom_start, cw, yb, yb + t, 255);
    }
    true
}

/// Heavy lines: light ones, twice as thick.
fn heavy(c: char, cw: usize, ch: usize, stride: usize, cov: &mut [u8]) -> bool {
    let arms = match c {
        '━' => L | R,
        '┃' => U | D,
        '┏' => R | D,
        '┓' => L | D,
        '┗' => U | R,
        _ => U | L, // ┛
    };
    let t = (((cw as f32 / 8.0).round() as usize).max(1) * 2)
        .min(cw / 2)
        .max(1);
    single(arms, t, cw, ch, stride, cov);
    true
}

/// One line of thickness `t` from the middle of the cell out along each arm.
fn single(arms: u8, t: usize, cw: usize, ch: usize, stride: usize, cov: &mut [u8]) {
    let has = |a: u8| arms & a != 0;
    let (x0, y0) = ((cw - t) / 2, (ch - t) / 2);
    if has(U) {
        fill(cov, stride, x0, x0 + t, 0, y0 + t, 255);
    }
    if has(D) {
        fill(cov, stride, x0, x0 + t, y0, ch, 255);
    }
    if has(L) {
        fill(cov, stride, 0, x0 + t, y0, y0 + t, 255);
    }
    if has(R) {
        fill(cov, stride, x0, cw, y0, y0 + t, 255);
    }
}

/// Block elements (U+2580..U+259F): halves, eighths, shades and quadrants.
fn block(c: char, cw: usize, ch: usize, stride: usize, cov: &mut [u8]) -> bool {
    let n = c as u32;
    if !(0x2580..=0x259F).contains(&n) {
        return false;
    }
    let eighth_h = |k: u32| (ch as f32 * k as f32 / 8.0).round() as usize;
    let eighth_w = |k: u32| (cw as f32 * k as f32 / 8.0).round() as usize;
    let (hx, hy) = (cw / 2, ch / 2);
    let quads = |cov: &mut [u8], ul: bool, ur: bool, ll: bool, lr: bool| {
        if ul {
            fill(cov, stride, 0, hx, 0, hy, 255);
        }
        if ur {
            fill(cov, stride, hx, cw, 0, hy, 255);
        }
        if ll {
            fill(cov, stride, 0, hx, hy, ch, 255);
        }
        if lr {
            fill(cov, stride, hx, cw, hy, ch, 255);
        }
    };
    match n {
        0x2580 => fill(cov, stride, 0, cw, 0, hy, 255),
        0x2581..=0x2588 => fill(cov, stride, 0, cw, ch - eighth_h(n - 0x2580), ch, 255),
        0x2589..=0x258F => fill(cov, stride, 0, eighth_w(8 - (n - 0x2588)), 0, ch, 255),
        0x2590 => fill(cov, stride, hx, cw, 0, ch, 255),
        0x2591 => fill(cov, stride, 0, cw, 0, ch, 64),
        0x2592 => fill(cov, stride, 0, cw, 0, ch, 128),
        0x2593 => fill(cov, stride, 0, cw, 0, ch, 192),
        0x2594 => fill(cov, stride, 0, cw, 0, eighth_h(1), 255),
        0x2595 => fill(cov, stride, cw - eighth_w(1), cw, 0, ch, 255),
        0x2596 => quads(cov, false, false, true, false),
        0x2597 => quads(cov, false, false, false, true),
        0x2598 => quads(cov, true, false, false, false),
        0x2599 => quads(cov, true, false, true, true),
        0x259A => quads(cov, true, false, false, true),
        0x259B => quads(cov, true, true, true, false),
        0x259C => quads(cov, true, true, false, true),
        0x259D => quads(cov, false, true, false, false),
        0x259E => quads(cov, false, true, true, false),
        _ => quads(cov, false, true, true, true), // 0x259F
    }
    true
}

/// Symbols no bundled font has, drawn with antialiased shapes.
fn symbol(c: char, cw: usize, ch: usize, stride: usize, cov: &mut [u8]) -> bool {
    // signed distance → coverage, one pixel of antialiasing
    let ink = |d: f32| ((0.5 - d).clamp(0.0, 1.0) * 255.0) as u8;
    let (fw, fh) = (cw as f32, ch as f32);
    let (cx, cy) = (fw / 2.0, fh * 0.55);
    let r = (fw * 0.42).min(fh * 0.3);
    let lw = (fw / 9.0).max(1.0);
    let shape: &dyn Fn(f32, f32) -> f32 = match c {
        // pause: two upright bars
        '⏸' => &|x: f32, y: f32| {
            let bar = |x0: f32| ((x - x0).abs() - lw).max((y - cy).abs() - r);
            bar(cx - r * 0.5).min(bar(cx + r * 0.5))
        },
        // stopwatch: a ring, a hand pointing up-right and a button on top
        '⏱' => &|x: f32, y: f32| {
            let (dx, dy) = (x - cx, y - cy);
            let ring = ((dx * dx + dy * dy).sqrt() - r).abs() - lw / 2.0;
            let t = ((dx * 0.7 - dy * 0.7) / r).clamp(0.0, 0.75);
            let hand = ((dx - t * r * 0.7).powi(2) + (dy + t * r * 0.7).powi(2)).sqrt() - lw / 2.0;
            let top = (x - cx).abs().max((y - (cy - r - lw * 1.5)).abs()) - lw;
            ring.min(hand).min(top)
        },
        _ => return false,
    };
    for y in 0..ch {
        for x in 0..cw {
            cov[y * stride + x] = ink(shape(x as f32 + 0.5, y as f32 + 0.5));
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_the_game_uses_renders() {
        let mut g = Glyphs::new(18.0);
        assert!(
            g.cw >= 8 && g.ch > g.cw,
            "cells are taller than wide: {}x{}",
            g.cw,
            g.ch
        );
        let (cw, ch) = (g.cw, g.ch);
        for c in "Aa£·×—•…←↑→↓↔⇐⇑⇒⇓∞≈≥⌂⏱⏸✦✶✺▤●━┃┏┓┗┛─│┌┐└┘├┤┬┴┼═║╔╗╚╝╠╣╦╩╬▀▁▄█▌▐░▒▓▙▣▪▲▶▼◆◈◎●★♣⚠✔✘"
            .chars()
        {
            let cov = g.get(c, false, 1);
            assert_eq!(cov.len(), cw * ch);
            assert!(cov.iter().any(|&a| a > 0), "{c} drew nothing");
        }
        // track joins across cells: a horizontal line reaches both edges on the same row
        let cov = g.get('═', false, 1).to_vec();
        let rows: Vec<usize> = (0..ch).filter(|&y| cov[y * cw] > 0).collect();
        assert_eq!(
            rows.len(),
            2 * ((cw as f32 / 8.0).round() as usize).max(1),
            "two lines"
        );
        assert!(rows.iter().all(|&y| cov[y * cw + cw - 1] > 0));
        // and a junction's vertical lines line up with a straight one's
        let v = g.get('║', false, 1).to_vec();
        let j = g.get('╬', false, 1).to_vec();
        assert_eq!(&v[..cw], &j[..cw], "top rows match");
    }

    #[test]
    fn zoomed_tiles_fill_their_block() {
        let mut g = Glyphs::new(18.0);
        let (cw, ch) = (g.cw, g.ch);
        for (cols, rows) in [(2, 1), (4, 2)] {
            let (w, h) = (cw * cols, ch * rows);
            // straight track runs edge to edge through the middle of the block
            let across = g.get_block('═', false, cols, rows).to_vec();
            assert!((0..h).any(|y| across[y * w] > 0 && across[y * w + w - 1] > 0));
            let down = g.get_block('║', false, cols, rows).to_vec();
            let inked: Vec<usize> = (0..w).filter(|&x| down[x] > 0).collect();
            let mid = (inked[0] + inked[inked.len() - 1]) as f32 / 2.0;
            assert!(
                (mid - w as f32 / 2.0).abs() <= 1.0,
                "track down the middle at {cols}x{rows}"
            );
            assert!(
                g.get_block('█', false, cols, rows)
                    .iter()
                    .all(|&a| a == 255),
                "a train fills it"
            );
            // letters grow with the block
            let s = g.get_block('S', true, cols, rows).to_vec();
            let tall = (0..h)
                .filter(|&y| s[y * w..(y + 1) * w].iter().any(|&a| a > 0))
                .count();
            assert!(tall > ch * rows / 2, "S is {tall}px tall in {h}px");
        }
    }
}

#[cfg(test)]
mod size_sweep {
    use super::*;

    /// Every text size the window can be zoomed to draws every glyph without panicking.
    #[test]
    fn every_size_draws_every_glyph() {
        // the hand-drawn ones (where the layout math lives) at every size, font glyphs at a few
        let drawn = "━┃┏┓┗┛─│┌┐└┘├┤┬┴┼═║╔╗╚╝╠╣╦╩╬▀▁▄█▌▐░▒▓▙▖▗▘▚▛▜▝▞▟⏱⏸";
        let font = "Aa£·×—•…←↑→↓⌂✦●▣▪▲▶▼◆◎★♣⚠✔✘O°~∩";
        let mut g = Glyphs::new(16.0);
        for px in 2..=80 {
            g.set_px(px as f32);
            let sizes = [4, 9, 40];
            let chars = drawn
                .chars()
                .chain(font.chars().filter(|_| sizes.contains(&px)));
            for c in chars {
                for bold in [false, true] {
                    for cols in [1, 2] {
                        g.get(c, bold, cols);
                    }
                }
            }
            let _ = g.underline_row();
        }
    }
}
