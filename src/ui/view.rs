//! Where the map is on screen: the camera, the map zoom, and the sums that
//! turn a tile into a screen cell and back. Everything that draws on the map
//! or reads the mouse over it goes through `View`, so the zoom only lives here.
//!
//! The map zoom is separate from the window's text size: it changes how many
//! cells one tile takes, so the side panels keep their size.
//! - `Far`: one cell shows two tiles, one above the other, as a `▀` half block
//!   (colors only, no glyphs). Tiles come out square, and a lot fits.
//! - `Normal`: one cell per tile (cells are about twice as tall as wide).
//! - `Close`: two cells across per tile, so tiles are about square.
//! - `Closer`: four cells across and two down.
//!
//! A tile's glyph (its track piece, station, train) sits in one cell of its
//! block (`MapZoom::glyph`); the others continue the track or show the ground.
//! The window goes further and stretches the glyph over the block (`MapBlock`).

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use crate::types::Pos;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum MapZoom {
    Far,
    #[default]
    Normal,
    Close,
    Closer,
}

impl MapZoom {
    pub const ALL: [MapZoom; 4] = [
        MapZoom::Far,
        MapZoom::Normal,
        MapZoom::Close,
        MapZoom::Closer,
    ];

    pub fn name(self) -> &'static str {
        match self {
            MapZoom::Far => "Far",
            MapZoom::Normal => "Normal",
            MapZoom::Close => "Close",
            MapZoom::Closer => "Closer",
        }
    }

    /// Screen columns per tile.
    pub fn cols(self) -> i32 {
        match self {
            MapZoom::Far | MapZoom::Normal => 1,
            MapZoom::Close => 2,
            MapZoom::Closer => 4,
        }
    }

    /// Screen rows per tile (`Far` packs two tiles into one row).
    pub fn rows(self) -> i32 {
        match self {
            MapZoom::Closer => 2,
            _ => 1,
        }
    }

    /// Tiles per screen row: 2 when far out, else 1.
    pub fn stack(self) -> i32 {
        if self == MapZoom::Far { 2 } else { 1 }
    }

    /// The cell in a tile's block that shows its glyph (column, row).
    pub fn glyph(self) -> (i32, i32) {
        match self {
            MapZoom::Closer => (1, 0),
            _ => (0, 0),
        }
    }

    pub fn closer(self) -> Option<MapZoom> {
        let i = MapZoom::ALL.iter().position(|z| *z == self)?;
        MapZoom::ALL.get(i + 1).copied()
    }

    pub fn farther(self) -> Option<MapZoom> {
        let i = MapZoom::ALL.iter().position(|z| *z == self)?;
        i.checked_sub(1).map(|i| MapZoom::ALL[i])
    }
}

/// One zoomed-in tile as the map drew it: its block of screen cells and what
/// went in them. The window paints it as one glyph stretched over the block,
/// as long as nothing has been drawn over those cells since.
#[derive(Clone, Debug)]
pub struct MapBlock {
    pub rect: Rect,
    /// The cells of `rect`, row by row; the tile's glyph is at `MapZoom::glyph`.
    pub cells: Vec<ratatui::buffer::Cell>,
    pub glyph: (u16, u16),
}

/// The map on screen: which tile is at the top-left (`cam`), how big tiles
/// are, and the screen area the map fills.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub zoom: MapZoom,
    pub cam: Pos,
    pub area: Rect,
}

impl View {
    /// How many tiles fit across and down.
    pub fn tiles(&self) -> (i32, i32) {
        let z = self.zoom;
        (
            (self.area.width as i32 / z.cols()).max(1),
            (self.area.height as i32 * z.stack() / z.rows()).max(1),
        )
    }

    /// The screen cell (absolute) where a tile at fractional map position
    /// (x, y) shows its glyph, if that's on the map area.
    pub fn cell_of(&self, x: f32, y: f32) -> Option<(u16, u16)> {
        let (sx, sy) = self.offset_of(x, y);
        (sx >= 0 && sy >= 0 && sx < self.area.width as i32 && sy < self.area.height as i32)
            .then(|| (self.area.x + sx as u16, self.area.y + sy as u16))
    }

    /// Like `cell_of`, relative to the map area and never refused (may be off screen).
    pub fn offset_of(&self, x: f32, y: f32) -> (i32, i32) {
        let z = self.zoom;
        let (gx, gy) = z.glyph();
        let col = ((x - self.cam.x as f32) * z.cols() as f32).round() as i32 + gx;
        let row =
            ((y - self.cam.y as f32) * z.rows() as f32 / z.stack() as f32).floor() as i32 + gy;
        (col, row)
    }

    /// The glyph cell of a tile, relative to the map area.
    pub fn offset(&self, p: Pos) -> (i32, i32) {
        let z = self.zoom;
        let (gx, gy) = z.glyph();
        (
            (p.x - self.cam.x) * z.cols() + gx,
            (p.y - self.cam.y).div_euclid(z.stack()) * z.rows() + gy,
        )
    }

    /// The tile under a screen cell (absolute), without checking the map's
    /// bounds; None off the map area. When far out it's the upper of the two.
    pub fn tile_at(&self, x: u16, y: u16) -> Option<Pos> {
        let a = self.area;
        if x < a.x || y < a.y || x >= a.x + a.width || y >= a.y + a.height {
            return None;
        }
        Some(self.tile_at_offset((x - a.x) as i32, (y - a.y) as i32))
    }

    /// The tile at a cell relative to the map area.
    pub fn tile_at_offset(&self, sx: i32, sy: i32) -> Pos {
        let z = self.zoom;
        Pos::new(
            self.cam.x + sx.div_euclid(z.cols()),
            self.cam.y + sy.div_euclid(z.rows()) * z.stack(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_and_cells_agree_at_every_zoom() {
        for z in MapZoom::ALL {
            let v = View {
                zoom: z,
                cam: Pos::new(10, 20),
                area: Rect::new(5, 3, 80, 30),
            };
            for p in [Pos::new(10, 20), Pos::new(17, 25), Pos::new(29, 31)] {
                let (sx, sy) = v.offset(p);
                let back = v.tile_at_offset(sx, sy);
                assert_eq!(back.x, p.x, "{z:?} {p:?}");
                assert_eq!(
                    back.y,
                    if z == MapZoom::Far {
                        p.y - (p.y - 20).rem_euclid(2)
                    } else {
                        p.y
                    },
                    "{z:?} {p:?}"
                );
                assert_eq!(
                    v.offset_of(p.x as f32, p.y as f32),
                    (sx, sy),
                    "{z:?} float and whole tiles match"
                );
            }
            let (tw, th) = v.tiles();
            assert_eq!(tw * z.cols(), 80 / z.cols() * z.cols());
            assert_eq!(th, 30 * z.stack() / z.rows());
        }
        assert_eq!(MapZoom::Normal.closer(), Some(MapZoom::Close));
        assert_eq!(MapZoom::Closer.closer(), None);
        assert_eq!(MapZoom::Far.farther(), None);
    }
}
