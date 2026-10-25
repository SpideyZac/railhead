//! Save and load: the whole `World` as JSON.

use std::path::Path;

use crate::world::World;

/// Writes to a temporary file first and then renames it, so a crash or a
/// full disk mid-save never leaves a half-written save behind.
pub fn save(w: &World, path: &Path) -> Result<(), String> {
    let json = serde_json::to_string(w).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn load(path: &Path) -> Result<World, String> {
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut w: World = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    w.rebuild_derived();
    Ok(w)
}

/// A one-line description of the save at `path` for the title screen, or
/// None if there is no loadable save there.
pub fn peek(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    let w: World = serde_json::from_str(&s).ok()?;
    let mins = |t: f64| format!("{:02}:{:02}", t as u64 / 60, t as u64 % 60);
    let when = match w.time_left() {
        _ if w.finished && w.franchise() => "franchise revoked".to_string(),
        _ if w.finished => "finished".to_string(),
        Some(left) => format!("{} left", mins(left)),
        None if w.franchise() => format!(
            "{} played · {} reviews passed",
            mins(w.time),
            w.stats.reviews_passed
        ),
        None => format!("{} played", mins(w.time)),
    };
    let mode = w.settings.mode.name();
    Some(format!(
        "{mode} · seed {} · {when} · Prosperity {:.0}",
        w.map.seed,
        w.prosperity()
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Settings;

    #[test]
    fn save_load_roundtrip_continues_identically() {
        let mut a = World::new(77, Settings::default());
        for _ in 0..600 {
            a.tick();
        }
        let dir = std::env::temp_dir().join("railhead_test_save.json");
        save(&a, &dir).unwrap();
        let mut b = load(&dir).unwrap();
        for _ in 0..600 {
            a.tick();
            b.tick();
        }
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        let _ = std::fs::remove_file(dir);
    }
}
