//! Player preferences that outlive a game: sound, palette, map marks, and
//! what was picked last on the title screen. Saved as `settings.json` in the
//! data folder. Missing or broken files just mean defaults.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    App,
    audio::Levels,
    palette::{Palette, PaletteKind},
    view::MapZoom,
};
use crate::{balance as B, map::MapSize, world::GameMode};

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prefs {
    /// The old three-way sound switch (0 = music and effects, 1 = effects
    /// only, 2 = muted). Only read, so settings from before the volume
    /// sliders keep what they meant.
    #[serde(default, skip_serializing)]
    pub sound: u8,
    /// Volume sliders in percent; None in files from before they existed.
    #[serde(default)]
    pub music: Option<u8>,
    #[serde(default)]
    pub effects: Option<u8>,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub colorblind: bool,
    #[serde(default = "yes")]
    pub terrain_marks: bool,
    #[serde(default = "yes")]
    pub labels: bool,
    #[serde(default = "yes")]
    pub mouse: bool,
    /// The keys panel on the left of the map.
    #[serde(default = "yes")]
    pub key_panel: bool,
    #[serde(default)]
    pub map_zoom: MapZoom,
    #[serde(default)]
    pub mode: GameMode,
    #[serde(default)]
    pub size: MapSize,
    /// Has this player ever started a game? First-timers land on Tutorial.
    #[serde(default)]
    pub played: bool,
    /// The game window's text size (logical px, 0 = default) and fullscreen.
    #[serde(default)]
    pub font_px: f32,
    #[serde(default)]
    pub fullscreen: bool,
}

impl Default for Prefs {
    fn default() -> Prefs {
        serde_json::from_str("{}").expect("every field has a default")
    }
}

impl Prefs {
    pub fn load(path: &Path) -> Prefs {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// The sound levels these preferences ask for.
    pub fn levels(&self) -> Levels {
        Levels {
            music: self
                .music
                .unwrap_or(if self.sound == 1 {
                    0
                } else {
                    B::MUSIC_PERCENT_DEFAULT
                })
                .min(100),
            sfx: self.effects.unwrap_or(B::SFX_PERCENT_DEFAULT).min(100),
            muted: self.muted || self.sound == 2,
        }
    }

    /// Put these preferences into effect.
    pub fn apply(&self, app: &mut App) {
        app.audio.set_levels(self.levels());
        app.pal = Palette::new(if self.colorblind {
            PaletteKind::ColorBlind
        } else {
            PaletteKind::Classic
        });
        app.terrain_marks = self.terrain_marks;
        app.labels = self.labels;
        app.mouse = self.mouse;
        app.key_panel = self.key_panel;
        app.map_zoom = self.map_zoom;
        app.title.mode = self.mode;
        app.title.size = self.size;
        if !self.played {
            app.title.row = super::TitleRow::Tutorial; // first-timers land on the Tutorial
        }
        app.font_px = self.font_px;
        app.fullscreen = self.fullscreen;
    }

    /// The preferences as they stand in the app now.
    pub fn from_app(app: &App) -> Prefs {
        let l = app.audio.levels;
        Prefs {
            sound: 0,
            music: Some(l.music),
            effects: Some(l.sfx),
            muted: l.muted,
            colorblind: app.pal.kind == PaletteKind::ColorBlind,
            terrain_marks: app.terrain_marks,
            labels: app.labels,
            mouse: app.mouse,
            key_panel: app.key_panel,
            map_zoom: app.map_zoom,
            mode: app.title.mode,
            size: app.title.size,
            played: app.played,
            font_px: app.font_px,
            fullscreen: app.fullscreen,
        }
    }
}

/// Save the current preferences (if this app has somewhere to save them).
pub fn save(app: &App) {
    if let Some(path) = &app.prefs_path
        && let Ok(json) = serde_json::to_string_pretty(&Prefs::from_app(app))
    {
        let _ = std::fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Settings;

    #[test]
    fn prefs_round_trip_and_tolerate_junk() {
        let path = std::env::temp_dir().join("rh_prefs_test.json");
        let mut app = App::new(
            Some("1".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_prefs_save.json"),
        );
        app.prefs_path = Some(path.clone());
        app.labels = false;
        app.audio.set_levels(Levels {
            music: 30,
            sfx: 90,
            muted: true,
        });
        app.map_zoom = MapZoom::Close;
        app.title.size = MapSize::Huge;
        app.played = true;
        save(&app);
        let back = Prefs::load(&path);
        assert!(
            !back.labels
                && back.size == MapSize::Huge
                && back.played
                && back.map_zoom == MapZoom::Close
        );
        assert_eq!(
            back.levels(),
            Levels {
                music: 30,
                sfx: 90,
                muted: true
            }
        );
        std::fs::write(&path, "{ not json").unwrap();
        assert!(Prefs::load(&path).labels, "broken file → defaults");
        std::fs::write(&path, "{}").unwrap();
        let p = Prefs::load(&path);
        assert!(
            p.labels && p.terrain_marks && p.key_panel && !p.played,
            "missing fields → defaults"
        );
        assert_eq!(p.levels(), Levels::default());
        // settings from before the sliders keep their meaning
        std::fs::write(&path, r#"{"sound": 1}"#).unwrap();
        assert_eq!(
            Prefs::load(&path).levels().music,
            0,
            "effects only: no music"
        );
        std::fs::write(&path, r#"{"sound": 2}"#).unwrap();
        assert!(Prefs::load(&path).levels().muted);
        let _ = std::fs::remove_file(path);
    }
}
