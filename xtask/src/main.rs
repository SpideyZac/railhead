//! Railhead's release tooling.
//!
//!   cargo xtask icons                 redraw assets/icons (PNG sizes, .ico, .icns)
//!   cargo xtask dist [--os OS] [--bin PATH] [--no-build]
//!                                     package a release into dist/ for windows,
//!                                     macos or linux (default: this computer's OS)
//!
//! The icon is pixel art drawn in code (a 32x32 grid), so it can be tweaked
//! here and redrawn without an image editor. Packages:
//!   windows  Railhead.exe (icon + version info built in by build.rs) in a .zip
//!   macos    Railhead.app (the game opens its own window), in a .zip
//!   linux    binary + .desktop launcher + icons + install.sh, in a .tar.gz
//! Every package carries the DejaVu font licence (the window's fonts are built in).

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let result = match args.first().map(String::as_str) {
        Some("icons") => icons(&root),
        Some("dist") => {
            let os = get("--os").unwrap_or_else(|| std::env::consts::OS.to_string());
            dist(&root, &os, get("--bin").map(PathBuf::from), args.iter().any(|a| a == "--no-build"))
        }
        _ => Err("usage: cargo xtask icons | cargo xtask dist [--os windows|macos|linux] [--bin PATH] [--no-build]".into()),
    };
    if let Err(e) = result {
        eprintln!("xtask: {e}");
        std::process::exit(1);
    }
}

type Rgba = [u8; 4];

/// The 32x32 master icon: a red steam engine on the rails under a puff of
/// smoke, on a dark rounded tile with a gold rim (the game's title colors).
fn master() -> Vec<Rgba> {
    const N: i32 = 32;
    let mut px = vec![[0u8; 4]; (N * N) as usize];
    let mut put = |x: i32, y: i32, c: Rgba| {
        if (0..N).contains(&x) && (0..N).contains(&y) {
            px[(y * N + x) as usize] = c;
        }
    };
    let rect =
        |put: &mut dyn FnMut(i32, i32, Rgba), x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba| {
            for y in y0..y1 {
                for x in x0..x1 {
                    put(x, y, c);
                }
            }
        };
    let disc = |put: &mut dyn FnMut(i32, i32, Rgba), cx: f32, cy: f32, r: f32, c: Rgba| {
        for y in 0..N {
            for x in 0..N {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    put(x, y, c);
                }
            }
        }
    };
    // tile: rounded square, gold rim, night-sky gradient
    let inside = |x: i32, y: i32, inset: i32| {
        let r = 6 - inset;
        let (lo, hi) = (inset, N - 1 - inset);
        if x < lo || x > hi || y < lo || y > hi {
            return false;
        }
        let cx = x.clamp(lo + r, hi - r);
        let cy = y.clamp(lo + r, hi - r);
        let (dx, dy) = ((x - cx) as f32, (y - cy) as f32);
        dx * dx + dy * dy <= (r as f32 + 0.5).powi(2)
    };
    for y in 0..N {
        for x in 0..N {
            if inside(x, y, 0) {
                put(x, y, [255, 190, 70, 255]);
            }
            if inside(x, y, 1) {
                let t = y as f32 / N as f32;
                let mix = |a: f32, b: f32| (a + (b - a) * t) as u8;
                put(
                    x,
                    y,
                    [mix(44.0, 16.0), mix(52.0, 18.0), mix(84.0, 30.0), 255],
                );
            }
        }
    }
    let dark = [34, 34, 42, 255];
    let red = [214, 62, 46, 255];
    let deep = [150, 38, 34, 255];
    let gold = [255, 200, 80, 255];
    let steel = [205, 205, 215, 255];
    let smoke = [226, 228, 236, 255];
    // smoke
    disc(&mut put, 22.5, 6.5, 2.6, smoke);
    disc(&mut put, 26.5, 4.5, 2.0, smoke);
    disc(&mut put, 18.5, 5.0, 1.6, [190, 194, 206, 255]);
    // rails and sleepers
    for x in 2..30 {
        if x % 4 < 2 {
            rect(&mut put, x, 27, x + 1, 29, [130, 92, 58, 255]);
        }
    }
    rect(&mut put, 2, 26, 30, 27, steel);
    // cab with a lit window and roof
    rect(&mut put, 5, 11, 12, 22, deep);
    rect(&mut put, 7, 13, 10, 16, [255, 228, 150, 255]);
    rect(&mut put, 4, 10, 13, 11, dark);
    // boiler with a gold band, chimney, dome and lamp
    rect(&mut put, 11, 15, 26, 22, red);
    rect(&mut put, 11, 15, 26, 16, [236, 96, 70, 255]);
    rect(&mut put, 17, 15, 18, 22, gold);
    rect(&mut put, 21, 10, 24, 15, dark);
    rect(&mut put, 20, 9, 25, 10, dark);
    rect(&mut put, 14, 13, 17, 15, gold);
    rect(&mut put, 26, 16, 27, 18, [255, 246, 170, 255]);
    // frame, cowcatcher, wheels
    rect(&mut put, 4, 22, 28, 23, dark);
    for (i, w) in [(0, 26), (1, 27), (2, 28)] {
        rect(&mut put, 26, 19 + i, w + 1, 20 + i, steel);
    }
    for cx in [8.5f32, 15.5, 22.5] {
        disc(&mut put, cx, 23.5, 2.6, dark);
        disc(&mut put, cx, 23.5, 1.0, steel);
    }
    px
}

/// Area-weighted resample of the 32x32 master to `size`x`size`: whole-number
/// upscales stay crisp pixel art, downscales are averaged.
fn resample(src: &[Rgba], size: u32) -> Vec<u8> {
    const N: u32 = 32;
    const SS: u32 = 8;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0u32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = ((x * SS + sx) * N / (size * SS)).min(N - 1);
                    let v = ((y * SS + sy) * N / (size * SS)).min(N - 1);
                    let p = src[(v * N + u) as usize];
                    // premultiply so transparent edges don't darken
                    let a = p[3] as u32;
                    for c in 0..3 {
                        acc[c] += p[c] as u32 * a;
                    }
                    acc[3] += a;
                }
            }
            let a = acc[3];
            let n = SS * SS;
            if a == 0 {
                out.extend([0, 0, 0, 0]);
            } else {
                out.extend([
                    (acc[0] / a) as u8,
                    (acc[1] / a) as u8,
                    (acc[2] / a) as u8,
                    (a / n) as u8,
                ]);
            }
        }
    }
    out
}

fn png_bytes(rgba: &[u8], size: u32) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, size, size);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::High);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(buf)
}

/// A Windows .ico holding PNG images (fine for Vista and later).
fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0, 0, 1, 0];
    out.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in images {
        let s = if *size >= 256 { 0 } else { *size as u8 };
        out.extend([s, s, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend(data);
    }
    out
}

/// A macOS .icns holding PNG images.
fn icns(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (size, data) in images {
        let kind: &[u8; 4] = match size {
            16 => b"icp4",
            32 => b"icp5",
            64 => b"icp6",
            128 => b"ic07",
            256 => b"ic08",
            512 => b"ic09",
            _ => continue,
        };
        body.extend(kind);
        body.extend((data.len() as u32 + 8).to_be_bytes());
        body.extend(data);
    }
    let mut out = b"icns".to_vec();
    out.extend((body.len() as u32 + 8).to_be_bytes());
    out.extend(body);
    out
}

fn icons(root: &Path) -> Result<(), String> {
    let dir = root.join("assets/icons");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let m = master();
    let mut images = vec![];
    for size in [16u32, 24, 32, 48, 64, 128, 256, 512] {
        let data = png_bytes(&resample(&m, size), size)?;
        fs::write(dir.join(format!("railhead-{size}.png")), &data).map_err(|e| e.to_string())?;
        images.push((size, data));
    }
    let ico_set: Vec<_> = images.iter().filter(|(s, _)| *s <= 256).cloned().collect();
    fs::write(dir.join("railhead.ico"), ico(&ico_set)).map_err(|e| e.to_string())?;
    fs::write(dir.join("railhead.icns"), icns(&images)).map_err(|e| e.to_string())?;
    println!("icons written to {}", dir.display());
    Ok(())
}

fn version(root: &Path) -> Result<String, String> {
    let toml = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    toml.lines()
        .find_map(|l| {
            l.strip_prefix("version = \"")
                .and_then(|v| v.strip_suffix('"'))
        })
        .map(str::to_string)
        .ok_or_else(|| "no version in Cargo.toml".into())
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed ({status})"))
    }
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(d) = to.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("copy {} → {}: {e}", from.display(), to.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg(unix)]
fn make_executable(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}
#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<(), String> {
    Ok(()) // the archive tool can't set Unix modes from Windows; install.sh / the user does
}

const README: &str = "RAILHEAD

A railway-network builder and train simulator.

STARTING
  Windows  double-click Railhead.exe
  macOS    double-click Railhead.app. The first time,
           right-click it and choose Open if macOS says it can't check it.
  Linux    run ./install.sh once to add Railhead to your applications menu,
           or just run ./railhead.
  Prefer a real terminal? Start it with: railhead --terminal

New here? Choose Tutorial on the title screen. In game, ? opens the Railway
Handbook and C lets you change any key.

Ctrl+= and Ctrl+- change the text size, Alt+Enter (or F11) is fullscreen.
Saves, settings and key bindings live in:
  Windows  %APPDATA%\\Railhead
  macOS    ~/Library/Application Support/Railhead
  Linux    ~/.local/share/railhead
";

fn dist(root: &Path, os: &str, bin: Option<PathBuf>, no_build: bool) -> Result<(), String> {
    let ver = version(root)?;
    let exe = if os == "windows" {
        "railhead.exe"
    } else {
        "railhead"
    };
    let bin = match bin {
        Some(b) => b,
        None => {
            if !no_build {
                let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
                run(Command::new(cargo).current_dir(root).args([
                    "build",
                    "--profile",
                    "dist",
                    "--package",
                    "railhead",
                ]))?;
            }
            root.join("target/dist").join(exe)
        }
    };
    if !bin.exists() {
        return Err(format!("{} not found", bin.display()));
    }
    let arch = std::env::consts::ARCH;
    let name = format!("Railhead-{ver}-{os}-{arch}");
    let out = root.join("dist");
    let stage = out.join(&name);
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let icons = root.join("assets/icons");
    let font_licence = root.join("assets/fonts/LICENSE-DejaVu.txt");
    let license = root.join("LICENSE");
    let archive;
    match os {
        "windows" => {
            copy(&bin, &stage.join("Railhead.exe"))?;
            write(&stage.join("README.txt"), &README.replace('\n', "\r\n"))?;
            copy(&font_licence, &stage.join("LICENSE-DejaVu.txt"))?;
            copy(&license, &stage.join("LICENSE.txt"))?;
            archive = out.join(format!("{name}.zip"));
            let _ = fs::remove_file(&archive);
            // bsdtar ships with Windows 10+ and writes zips with -a
            run(Command::new("tar")
                .current_dir(&out)
                .arg("-a")
                .arg("-cf")
                .arg(&archive)
                .arg(&name))?;
        }
        "macos" => {
            let app = stage.join("Railhead.app/Contents");
            copy(&bin, &app.join("MacOS/railhead"))?;
            make_executable(&app.join("MacOS/railhead"))?;
            copy(
                &icons.join("railhead.icns"),
                &app.join("Resources/railhead.icns"),
            )?;
            write(&app.join("Info.plist"), &info_plist(&ver))?;
            write(&stage.join("README.txt"), README)?;
            copy(&font_licence, &stage.join("LICENSE-DejaVu.txt"))?;
            copy(&license, &stage.join("LICENSE.txt"))?;
            archive = out.join(format!("{name}.zip"));
            let _ = fs::remove_file(&archive);
            if cfg!(target_os = "macos") {
                run(Command::new("ditto")
                    .current_dir(&out)
                    .args(["-c", "-k", "--keepParent"])
                    .arg(&name)
                    .arg(&archive))?;
            } else {
                run(Command::new("tar")
                    .current_dir(&out)
                    .arg("-a")
                    .arg("-cf")
                    .arg(&archive)
                    .arg(&name))?;
            }
        }
        "linux" => {
            copy(&bin, &stage.join("railhead"))?;
            make_executable(&stage.join("railhead"))?;
            for size in [16, 24, 32, 48, 64, 128, 256, 512] {
                copy(
                    &icons.join(format!("railhead-{size}.png")),
                    &stage.join(format!("icons/hicolor/{size}x{size}/apps/railhead.png")),
                )?;
            }
            write(&stage.join("railhead.desktop"), &desktop_entry(&ver))?;
            write(&stage.join("install.sh"), LINUX_INSTALL)?;
            make_executable(&stage.join("install.sh"))?;
            write(&stage.join("README.txt"), README)?;
            copy(&font_licence, &stage.join("LICENSE-DejaVu.txt"))?;
            copy(&license, &stage.join("LICENSE.txt"))?;
            archive = out.join(format!("{name}.tar.gz"));
            run(Command::new("tar")
                .current_dir(&out)
                .arg("-czf")
                .arg(&archive)
                .arg(&name))?;
        }
        other => return Err(format!("unknown --os {other} (windows, macos or linux)")),
    }
    println!("packaged {}", archive.display());
    Ok(())
}

fn info_plist(ver: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Railhead</string>
    <key>CFBundleDisplayName</key><string>Railhead</string>
    <key>CFBundleIdentifier</key><string>com.railhead.game</string>
    <key>CFBundleVersion</key><string>{ver}</string>
    <key>CFBundleShortVersionString</key><string>{ver}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>railhead</string>
    <key>CFBundleIconFile</key><string>railhead.icns</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.simulation-games</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
    )
}

fn desktop_entry(ver: &str) -> String {
    format!(
        "[Desktop Entry]
Type=Application
Version=1.0
Name=Railhead
GenericName=Railway Builder
Comment=Build and manage railway networks (v{ver})
Exec=railhead
Icon=railhead
Terminal=false
StartupWMClass=railhead
Categories=Game;Simulation;
Keywords=train;railway;rail;transport;
"
    )
}

const LINUX_INSTALL: &str = r#"#!/bin/sh
# Installs Railhead for this user: ~/.local/bin, the icon theme and the applications menu.
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
mkdir -p "$BIN" "$DATA/applications"
install -m 755 "$HERE/railhead" "$BIN/railhead"
for d in "$HERE"/icons/hicolor/*/apps; do
    size="$(basename "$(dirname "$d")")"
    mkdir -p "$DATA/icons/hicolor/$size/apps"
    install -m 644 "$d/railhead.png" "$DATA/icons/hicolor/$size/apps/railhead.png"
done
sed "s|^Exec=railhead|Exec=$BIN/railhead|" "$HERE/railhead.desktop" > "$DATA/applications/railhead.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$DATA/icons/hicolor" || true
echo "Railhead installed. Find it in your applications menu, or run: $BIN/railhead"
"#;
