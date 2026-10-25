//! Everything that depends on the computer the game runs on: where saves
//! live, the console and terminal window, and what happens on a crash. The sim never
//! uses this; only `main` and `ui::run` do.

use std::path::{Path, PathBuf};

/// The folder for saves, settings, key bindings and crash logs:
///   Windows  %APPDATA%\Railhead
///   macOS    ~/Library/Application Support/Railhead
///   Linux    $XDG_DATA_HOME/railhead (~/.local/share/railhead)
/// `RAILHEAD_HOME` overrides it, and a file named `portable` next to the
/// game puts everything beside the game instead (for USB sticks and the like).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("RAILHEAD_HOME") {
        return PathBuf::from(d);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
        && dir.join("portable").exists()
    {
        return dir;
    }
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let home = env("HOME")
        .or_else(|| env("USERPROFILE"))
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(windows) {
        env("APPDATA")
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("Railhead")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Railhead")
    } else {
        env("XDG_DATA_HOME")
            .unwrap_or_else(|| home.join(".local/share"))
            .join("railhead")
    }
}

/// Make the data folder, and bring over saves and key bindings that older
/// versions wrote into the current directory (only if there's no newer one).
pub fn prepare_data_dir(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    for (old, new) in [
        ("railhead_save.json", "save.json"),
        ("railhead_keys.json", "keys.json"),
    ] {
        let (old, new) = (Path::new(old), dir.join(new));
        if old.exists() && !new.exists() {
            let _ = std::fs::copy(old, &new);
        }
    }
}

/// True when the game has a console window all to itself, i.e. it was
/// double-clicked rather than started from a shell. Then the window closes
/// as soon as we exit, so crash messages need a pause to be read.
pub fn owns_window() -> bool {
    #[cfg(windows)]
    {
        let mut ids = [0u32; 4];
        // SAFETY: the buffer is valid for `ids.len()` entries.
        let n = unsafe { win::GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) };
        n == 1
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Name the window. Nothing else: the game never maximizes or resizes the
/// window. Windows Terminal obeys a maximize (then the player can't resize it),
/// and a size request changes the console *under* Windows Terminal without
/// changing the window, so the game would lay itself out for the wrong size.
/// The game fits whatever window it gets.
pub fn setup_window() {
    use ratatui::crossterm::{execute, terminal};
    let _ = execute!(std::io::stdout(), terminal::SetTitle("Railhead"));
}

/// On a panic: write a crash log to the data folder and say where it is. In
/// a terminal, put the terminal back first and wait for Enter if the window
/// would vanish; in the game's own window, say it in a message box.
pub fn install_crash_handler(dir: PathBuf, windowed: bool) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !windowed {
            let _ = ratatui::crossterm::execute!(
                std::io::stdout(),
                ratatui::crossterm::event::DisableMouseCapture
            );
        }
        previous(info); // ratatui's hook: puts the terminal back to normal
        let log = dir.join("crash.log");
        let text = format!(
            "Railhead {} crashed.\n{info}\n\n{}\n",
            env!("CARGO_PKG_VERSION"),
            std::backtrace::Backtrace::force_capture()
        );
        let saved = std::fs::write(&log, text).is_ok();
        let mut say = String::from("Railhead hit a bug and had to stop. Sorry about that!\n");
        if saved {
            say += &format!(
                "Details are in {} — sending it along helps fix it.\n",
                log.display()
            );
        }
        say += "Your last save (and autosave) are safe.";
        if windowed {
            message_box("Railhead", &say);
            return;
        }
        eprintln!("\n{say}");
        if owns_window() {
            eprintln!("\nPress Enter to close.");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
    }));
}

/// Show a message in a native dialog (Windows); elsewhere print it.
pub fn message_box(title: &str, text: &str) {
    #[cfg(windows)]
    {
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (t, m) = (wide(title), wide(text));
        // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
        unsafe { win::MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), 0x10) };
    }
    #[cfg(not(windows))]
    eprintln!("{title}: {text}");
}

/// Windows builds are GUI programs (so no console window opens behind the
/// game), which means they start without a console. Text output (`--help`,
/// `--sim`, `--snapshot`) goes to the console of the shell that started us,
/// if there is one. `--terminal` play needs a console of its own, because
/// shells don't wait for GUI programs and would fight the game for the keys.
pub fn attach_console(own: bool) {
    #[cfg(windows)]
    {
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
        // SAFETY: plain Win32 calls without pointers.
        unsafe {
            if own {
                win::FreeConsole();
                win::AllocConsole();
            } else if win::GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
                // stdout isn't a file or pipe: borrow the shell's console
                win::AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = own;
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        pub fn AttachConsole(pid: u32) -> i32;
        pub fn AllocConsole() -> i32;
        pub fn FreeConsole() -> i32;
        pub fn GetStdHandle(id: u32) -> *mut c_void;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn MessageBoxW(
            hwnd: *mut c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }
}
