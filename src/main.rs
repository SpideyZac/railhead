//! Railhead - a terminal-style railway-network builder.
//!
//! Usage:
//!   railhead [--terminal] [--seed S] [--size small|normal|large|huge] [--sandbox|--franchise] [--mute] [--no-mouse] [--save FILE] [--keys FILE]
//!   railhead --sim GAMES [--ticks N] [--seed FIRST] [--policy idle|builder|rail|both|all] [--franchise] [-v]

// Windows: a GUI program, so double-clicking opens only the game's window
// (`platform::attach_console` gives text output a console when it needs one).
#![cfg_attr(
    all(windows, feature = "window", not(test)),
    windows_subsystem = "windows"
)]

mod balance;
mod headless;
mod map;
mod path;
mod platform;
mod rng;
pub mod save;
mod types;
mod ui;
mod world;

use std::path::PathBuf;

use world::{GameMode, Settings};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let terminal = args.iter().any(|a| a == "--terminal") || !cfg!(feature = "window");
    let text_only = [
        "--help",
        "-h",
        "--sim",
        "--make-save",
        "--sound-test",
        "--music-wav",
        "--snapshot",
    ]
    .iter()
    .any(|f| args.iter().any(|a| a == f));
    if terminal || text_only {
        platform::attach_console(terminal && !text_only);
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", USAGE);
        return Ok(());
    }

    let mode = if args.iter().any(|a| a == "--sandbox") {
        GameMode::Sandbox
    } else if args.iter().any(|a| a == "--franchise") {
        GameMode::Franchise
    } else {
        GameMode::Charter
    };
    let size = match get("--size").map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("small") => map::MapSize::Small,
        Some("large") => map::MapSize::Large,
        Some("huge") => map::MapSize::Huge,
        _ => map::MapSize::Normal,
    };

    if get("--sim").is_some() || get("--make-save").is_some() {
        let policies = match get("--policy").as_deref() {
            Some("idle") => vec![headless::Policy::Idle],
            Some("builder") => vec![headless::Policy::Builder],
            Some("rail") => vec![headless::Policy::Rail],
            Some("all") => vec![
                headless::Policy::Idle,
                headless::Policy::Builder,
                headless::Policy::Rail,
            ],
            _ => vec![headless::Policy::Idle, headless::Policy::Builder],
        };
        let opts = headless::SimOpts {
            games: get("--sim").and_then(|g| g.parse().ok()).unwrap_or(10),
            ticks: get("--ticks")
                .and_then(|s| s.parse().ok())
                .unwrap_or(60 * 60 * balance::TICKS_PER_SEC),
            first_seed: get("--seed").map(|s| rng::parse_seed(&s)).unwrap_or(1),
            policies,
            verbose: args.iter().any(|a| a == "-v"),
            size,
            mode,
        };
        if let Some(path) = get("--make-save") {
            // a bot plays up to --ticks, then the game is saved: handy for trying late-game screens
            let w = headless::play(opts.first_seed, &opts, *opts.policies.last().unwrap());
            match save::save(&w, std::path::Path::new(&path)) {
                Ok(()) => println!(
                    "saved {path} at {:.0} game-minutes (load it with --save {path}, then Continue)",
                    w.time / 60.0
                ),
                Err(e) => eprintln!("couldn't save: {e}"),
            }
            return Ok(());
        }
        headless::run(&opts);
        return Ok(());
    }

    if args.iter().any(|a| a == "--sound-test") {
        println!("Playing every sound effect, then some music...");
        if !ui::audio::Audio::demo() {
            println!("No sound device (or built without the audio feature).");
        }
        return Ok(());
    }

    if let Some(dir) = get("--music-wav") {
        match ui::audio::write_previews(std::path::Path::new(&dir)) {
            Ok(files) => files.iter().for_each(|f| println!("wrote {}", f.display())),
            Err(e) => eprintln!("couldn't write the previews: {e}"),
        }
        return Ok(());
    }

    if let Some(seed) = get("--snapshot") {
        let ticks = args
            .iter()
            .position(|a| a == "--snapshot")
            .and_then(|i| args.get(i + 2))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let overlay = get("--overlay").and_then(|s| s.parse().ok()).unwrap_or(0);
        let width = get("--width").and_then(|s| s.parse().ok()).unwrap_or(150);
        let height = get("--height").and_then(|s| s.parse().ok()).unwrap_or(46);
        print!(
            "{}",
            ui::snapshot(
                &seed,
                ticks,
                width,
                height,
                overlay,
                Settings { mode, size }
            )
        );
        return Ok(());
    }

    let data = platform::data_dir();
    platform::prepare_data_dir(&data);
    let save_path = get("--save")
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("save.json"));
    let mut app = ui::App::new(get("--seed"), Settings { mode, size }, save_path);
    let prefs_path = data.join("settings.json");
    ui::prefs::Prefs::load(&prefs_path).apply(&mut app);
    app.prefs_path = Some(prefs_path);
    app.autosave_path = Some(data.join("autosave.json"));
    // the command line beats remembered choices
    if get("--size").is_some() {
        app.title.size = size;
    }
    if mode != GameMode::Charter {
        app.title.mode = mode;
        app.title.row = ui::TitleRow::Play;
    }
    app.keys_path = get("--keys")
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("keys.json"));
    app.data_dir = data;
    let (keymap, problems) = ui::keys::Keymap::load(&app.keys_path);
    app.keys = keymap;
    if !problems.is_empty() {
        app.flash(
            format!("Key bindings: {}", problems.join("; ")),
            world::AlertLevel::Warn,
        );
    }
    if args.iter().any(|a| a == "--no-mouse") {
        app.mouse = false;
    }
    if args.iter().any(|a| a == "--mute") {
        app.audio.levels.muted = true;
    }
    app.played = app
        .prefs_path
        .as_ref()
        .is_some_and(|p| ui::prefs::Prefs::load(p).played);
    app.records = ui::records::Records::load(&app.data_dir.join("records.json"));
    app.refresh_save_info();
    #[cfg(feature = "window")]
    if !terminal {
        // no display (e.g. over SSH): play in the terminal instead
        match ui::window::run(app) {
            Ok(r) => return r,
            Err(back) => return ui::run(*back),
        }
    }
    ui::run(app)
}

const USAGE: &str = "RAILHEAD

Play:
  railhead [--terminal] [--seed S] [--size small|normal|large|huge] [--sandbox] [--mute] [--no-mouse] [--save FILE] [--keys FILE]
  (title screen: Tab picks Charter/Franchise/Sandbox, Ctrl+D = today's daily seed;
   in game, M mutes / unmutes; Settings (title screen or Esc menu) has the
   music and effects volumes, text size, map zoom and display options;
   C (or ? on the title screen) opens Controls to rebind any key, saved to
   keys.json in the data folder, or --keys FILE)
  The game opens in its own window; --terminal plays in the terminal instead.
  The mouse wheel (or . and ,) zooms the map. In the window: Ctrl+= / Ctrl+- /
  Ctrl+0 change the text size, Alt+Enter or F11 toggles fullscreen.

Balance harness (no UI):
  railhead --sim GAMES [--ticks N] [--seed FIRST] [--policy idle|builder|rail|both|all] [-v]
  e.g. railhead --sim 20 --ticks 36000 --policy both
  railhead --make-save FILE [--ticks N] [--seed S] [--policy P]   (a bot plays, then saves)

Check your speakers: railhead --sound-test
Listen to the soundtrack as WAV files: railhead --music-wav DIR

Seeds can be numbers or words (\"railhead --seed banana\").";
