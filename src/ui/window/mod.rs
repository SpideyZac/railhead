//! The game's own window: the same character grid the terminal shows, drawn
//! into a native window with the Railhead icon, so double-clicking the game
//! opens Railhead itself rather than a terminal running it.
//!
//! ratatui renders into a `TestBackend` exactly as it would for a terminal;
//! this module paints that buffer's cells with `glyphs` (a software renderer,
//! presented with `softbuffer`) and turns `winit` input into the crossterm
//! events `ui::handle_event` already understands. Nothing else in the game
//! knows which front end it runs in.
//!
//! Text size: the game's `zoom_in` / `zoom_out` / `zoom_reset` actions (Ctrl+= /
//! Ctrl+- / Ctrl+0 by default) and the Esc menu set `App::zoom`; `Win::zoom`
//! steps through `ZOOM_STEPS` and refuses sizes where 80x24 cells no longer
//! fit. Ctrl+wheel zooms directly. Alt+Enter or F11 toggles fullscreen.

mod glyphs;

use std::{
    num::NonZeroU32,
    rc::Rc,
    time::{Duration, Instant},
};

use glyphs::Glyphs;
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::{Buffer, Cell},
    crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton,
        MouseEvent, MouseEventKind,
    },
    style::{Color, Modifier},
};
use unicode_width::UnicodeWidthStr;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode as PhysCode, ModifiersState, NamedKey, PhysicalKey},
    platform::modifier_supplement::KeyEventExtModifierSupplement,
    window::{Fullscreen, Icon, Window, WindowId},
};

use super::{App, Clock, FRAME_MS, Screen, draw, handle_event, step};

/// Text size in logical pixels (scaled by the screen's DPI), and its limits.
pub const FONT_DEFAULT: f32 = super::TEXT_DEFAULT;
/// The sizes the zoom keys step through: one pixel at a time while small,
/// bigger jumps as it grows, so each press looks like a change.
const ZOOM_STEPS: [f32; 16] = [
    10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 32.0, 36.0, 40.0,
];
const FONT_MIN: f32 = ZOOM_STEPS[0];
const FONT_MAX: f32 = ZOOM_STEPS[ZOOM_STEPS.len() - 1];
/// The window asks for about this many cells at first (less on small screens).
const START_CELLS: (f64, f64) = (160.0, 48.0);
/// The smallest window, in cells.
const MIN_CELLS: (f64, f64) = (60.0, 20.0);
/// Colors for cells that leave them at the default.
const DEFAULT_FG: (u8, u8, u8) = (210, 210, 205);
const DEFAULT_BG: (u8, u8, u8) = (14, 15, 19);

static ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/railhead-64.png");

/// Play in a window. Gives the app back if no window can be opened (no
/// display, e.g. over SSH), so the caller can fall back to the terminal.
pub fn run(mut app: App) -> Result<std::io::Result<()>, Box<App>> {
    app.releases = true; // the window always says when a key is let go
    let Ok(event_loop) = EventLoop::new() else {
        return Err(Box::new(app));
    };
    app.font_px = remembered_size(app.font_px);
    let px = if app.font_px > 0.0 {
        app.font_px
    } else {
        FONT_DEFAULT
    };
    let mut win = Win {
        app,
        window: None,
        surface: None,
        glyphs: Glyphs::new(px),
        term: Terminal::new(TestBackend::new(START_CELLS.0 as u16, START_CELLS.1 as u16))
            .expect("test backend"),
        clock: Clock::new(),
        pixels: vec![],
        size: (0, 0),
        grid: (0, 0),
        origin: (0, 0),
        shown: None,
        mods: ModifiersState::empty(),
        held: None,
        wheel: 0.0,
        scale: 1.0,
        next_frame: Instant::now(),
        failed: false,
        error: None,
    };
    let ran = event_loop.run_app(&mut win);
    if win.failed {
        return Err(Box::new(win.app));
    }
    Ok(match (ran, win.error) {
        (_, Some(e)) => Err(e),
        (Err(e), None) => Err(std::io::Error::other(e)),
        (Ok(()), None) => Ok(()),
    })
}

struct Win {
    app: App,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    glyphs: Glyphs,
    term: Terminal<TestBackend>,
    clock: Clock,
    /// What's on screen (0RGB), the window's size in pixels, the grid in
    /// cells and where the grid starts (it is centered in the window).
    pixels: Vec<u32>,
    size: (usize, usize),
    grid: (u16, u16),
    origin: (usize, usize),
    /// The frame `pixels` shows, to repaint only rows that changed.
    shown: Option<Buffer>,
    mods: ModifiersState,
    /// The mouse button held down, if any: moving the mouse then drags it.
    held: Option<MouseButton>,
    /// Touchpad scrolling arrives in pixels; this gathers it into lines.
    wheel: f64,
    scale: f64,
    next_frame: Instant,
    /// The window couldn't be made: fall back to the terminal.
    failed: bool,
    error: Option<std::io::Error>,
}

impl ApplicationHandler for Win {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        match self.open(el) {
            Ok(()) => {
                self.app.audio = super::audio::Audio::open(self.app.audio.levels);
                self.app.windowed = true;
                crate::platform::install_crash_handler(self.app.data_dir.clone(), true);
            }
            Err(_) => {
                self.failed = true;
                el.exit();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // closing the window is quitting: keep the game, like the Quit menu does
                if self.app.screen == Screen::Game {
                    self.app.autosave();
                }
                el.exit();
            }
            WindowEvent::Resized(s) => self.resize(s),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale = scale_factor;
                self.set_font(self.font_px());
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::Focused(false) => {
                self.mods = ModifiersState::empty();
                self.held = None;
                self.app.dozer_held = false;
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if !self.window_key(&event)
                    && let Some(mut k) = key_event(&event, self.mods)
                {
                    if event.repeat {
                        k.kind = KeyEventKind::Repeat;
                    }
                    handle_event(&mut self.app, Event::Key(k));
                }
                self.redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // let-go keys matter for holding one down (the bulldozer)
                if let Some(mut k) = key_event(&event, self.mods) {
                    k.kind = KeyEventKind::Release;
                    handle_event(&mut self.app, Event::Key(k));
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let kind = match self.held {
                    Some(b) => MouseEventKind::Drag(b),
                    None => MouseEventKind::Moved,
                };
                self.mouse(kind, position);
            }
            WindowEvent::CursorLeft { .. } => self.app.mouse_pos = None,
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };
                if state == ElementState::Pressed {
                    self.held = Some(button);
                } else if self.held == Some(button) {
                    self.held = None;
                }
                let kind = if state == ElementState::Pressed {
                    MouseEventKind::Down(button)
                } else {
                    MouseEventKind::Up(button)
                };
                if let Some(p) = self.pointer() {
                    self.mouse(kind, p);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(p) => {
                        self.wheel += p.y / self.glyphs.ch as f64;
                        let whole = self.wheel.trunc();
                        self.wheel -= whole;
                        whole
                    }
                };
                if lines == 0.0 {
                    return;
                }
                if self.mods.control_key() {
                    self.zoom(lines.signum() as i32);
                } else if let Some(p) = self.pointer() {
                    let kind = if lines > 0.0 {
                        MouseEventKind::ScrollUp
                    } else {
                        MouseEventKind::ScrollDown
                    };
                    self.mouse(kind, p);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.paint() {
                    self.error = Some(e);
                    el.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.app.quit {
            el.exit();
            return;
        }
        // the zoom keys and menu rows ask; the window knows the screen, so it decides
        if let Some(z) = self.app.zoom.take() {
            self.zoom(z);
        }
        if std::mem::take(&mut self.app.toggle_fullscreen) {
            self.toggle_fullscreen();
        }
        let now = Instant::now();
        if now >= self.next_frame {
            step(&mut self.app, &mut self.clock);
            self.redraw();
            let frame = Duration::from_millis(FRAME_MS);
            self.next_frame += frame;
            if self.next_frame <= now {
                self.next_frame = now + frame; // fell behind: don't try to catch up
            }
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

impl Win {
    fn open(&mut self, el: &ActiveEventLoop) -> Result<(), Box<dyn std::error::Error>> {
        let scale = el
            .primary_monitor()
            .map(|m| m.scale_factor())
            .unwrap_or(1.0);
        self.glyphs.set_px(self.font_px_at(scale));
        let (cw, ch) = (self.glyphs.cw as f64 / scale, self.glyphs.ch as f64 / scale);
        // about START_CELLS, but never more than 90% of the screen
        let screen: LogicalSize<f64> = el
            .primary_monitor()
            .map(|m| m.size().to_logical(scale))
            .unwrap_or(LogicalSize::new(1280.0, 800.0));
        let w = (START_CELLS.0 * cw).min(screen.width * 0.9);
        let h = (START_CELLS.1 * ch).min(screen.height * 0.85);
        let mut attrs = Window::default_attributes()
            .with_title("Railhead")
            .with_inner_size(LogicalSize::new(w, h))
            .with_min_inner_size(LogicalSize::new(MIN_CELLS.0 * cw, MIN_CELLS.1 * ch))
            .with_window_icon(icon());
        if self.app.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_taskbar_icon(icon());
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // matches StartupWMClass in railhead.desktop, so docks show our icon
            use winit::platform::wayland::WindowAttributesExtWayland;
            attrs = WindowAttributesExtWayland::with_name(attrs, "railhead", "railhead");
        }
        let window = Rc::new(el.create_window(attrs)?);
        let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
        self.surface =
            Some(softbuffer::Surface::new(&context, window.clone()).map_err(|e| e.to_string())?);
        self.scale = window.scale_factor();
        self.glyphs.set_px(self.font_px_at(self.scale));
        let size = window.inner_size();
        self.window = Some(window);
        self.resize(size);
        Ok(())
    }

    /// The text size in logical pixels.
    fn font_px(&self) -> f32 {
        if self.app.font_px > 0.0 {
            self.app.font_px
        } else {
            FONT_DEFAULT
        }
    }

    fn font_px_at(&self, scale: f64) -> f32 {
        (self.font_px() as f64 * scale).round() as f32
    }

    /// Change the text size (remembered in settings) and refit the grid.
    fn set_font(&mut self, px: f32) {
        let px = px.clamp(FONT_MIN, FONT_MAX);
        if (px - FONT_DEFAULT).abs() < 0.01 {
            self.app.font_px = 0.0;
        } else {
            self.app.font_px = px;
        }
        super::prefs::save(&self.app);
        self.glyphs.set_px(self.font_px_at(self.scale));
        if let Some(w) = &self.window {
            let s = w.inner_size();
            self.resize(s);
        }
    }

    fn resize(&mut self, s: PhysicalSize<u32>) {
        let (w, h) = (s.width.max(1) as usize, s.height.max(1) as usize);
        self.size = (w, h);
        self.pixels = vec![pack(DEFAULT_BG); w * h];
        let cols = (w / self.glyphs.cw).clamp(1, u16::MAX as usize) as u16;
        let rows = (h / self.glyphs.ch).clamp(1, u16::MAX as usize) as u16;
        self.grid = (cols, rows);
        // saturating: a minimized window can be smaller than one cell
        self.origin = (
            w.saturating_sub(cols as usize * self.glyphs.cw) / 2,
            h.saturating_sub(rows as usize * self.glyphs.ch) / 2,
        );
        self.term.backend_mut().resize(cols, rows);
        self.shown = None;
        if let Some(surface) = self.surface.as_mut() {
            let _ = surface.resize(
                NonZeroU32::new(w as u32).unwrap(),
                NonZeroU32::new(h as u32).unwrap(),
            );
        }
        self.redraw();
    }

    fn redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Keys that belong to the window, not the game. True if it was one.
    fn window_key(&mut self, e: &winit::event::KeyEvent) -> bool {
        let fullscreen = matches!(e.logical_key, Key::Named(NamedKey::F11))
            || (self.mods.alt_key() && matches!(e.logical_key, Key::Named(NamedKey::Enter)));
        if fullscreen {
            if !e.repeat {
                self.toggle_fullscreen();
            }
            return true;
        }
        // the text size keys are the game's own (rebindable) actions: see `zoom`
        false
    }

    fn toggle_fullscreen(&mut self) {
        self.app.fullscreen = !self.app.fullscreen;
        super::prefs::save(&self.app);
        if let Some(w) = &self.window {
            w.set_fullscreen(self.app.fullscreen.then_some(Fullscreen::Borderless(None)));
        }
    }

    /// Step the text size up (+1), down (-1) or back to normal (0), never so
    /// big that the game's smallest screen (80x24 cells) stops fitting, and
    /// say what happened.
    fn zoom(&mut self, dir: i32) {
        let km = self.app.keys.clone();
        let keys = "{zoom_in} bigger · {zoom_out} smaller · {zoom_reset} normal";
        let Some(px) = zoom_step(self.font_px(), dir) else {
            let end = if dir > 0 { "biggest" } else { "smallest" };
            self.app.flash(
                km.fill(&format!("That's the {end} text there is. {keys}")),
                crate::world::AlertLevel::Info,
            );
            return;
        };
        if dir > 0 && !self.fits(px) {
            let msg = "That's as big as fits: the game needs 80×24 cells. Make the window bigger, or go fullscreen (F11 or Alt+Enter).";
            self.app.flash(msg, crate::world::AlertLevel::Info);
            return;
        }
        self.set_font(px);
        let (c, r) = self.grid;
        self.app.flash(km.fill(&format!("Text size {px:.0} ({c}×{r} cells). {keys}. The map has its own zoom: {{map_zoom_in}} {{map_zoom_out}} or the wheel.")), crate::world::AlertLevel::Info);
    }

    /// Would the game's smallest screen fit in the window at this text size?
    fn fits(&self, px: f32) -> bool {
        let (cw, ch) = self
            .glyphs
            .cell_size((px as f64 * self.scale).round() as f32);
        self.size.0 / cw >= super::draw::MIN_W as usize
            && self.size.1 / ch >= super::draw::MIN_H as usize
    }

    fn pointer_cell(&self, p: PhysicalPosition<f64>) -> (u16, u16) {
        let x = (p.x.max(0.0) as usize).saturating_sub(self.origin.0) / self.glyphs.cw;
        let y = (p.y.max(0.0) as usize).saturating_sub(self.origin.1) / self.glyphs.ch;
        (
            (x as u16).min(self.grid.0 - 1),
            (y as u16).min(self.grid.1 - 1),
        )
    }

    fn pointer(&self) -> Option<PhysicalPosition<f64>> {
        let (x, y) = self.app.mouse_pos?;
        Some(PhysicalPosition::new(
            (self.origin.0 + x as usize * self.glyphs.cw) as f64,
            (self.origin.1 + y as usize * self.glyphs.ch) as f64,
        ))
    }

    fn mouse(&mut self, kind: MouseEventKind, p: PhysicalPosition<f64>) {
        let (column, row) = self.pointer_cell(p);
        handle_event(
            &mut self.app,
            Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: modifiers(self.mods),
            }),
        );
        self.redraw();
    }

    /// Draw the game into the grid, paint the rows that changed, and show them.
    fn paint(&mut self) -> std::io::Result<()> {
        let app = &mut self.app;
        let Ok(frame) = self.term.draw(|f| draw::draw(f, app));
        let buf = frame.buffer;
        let (cw, ch) = (self.glyphs.cw, self.glyphs.ch);
        let stride = self.size.0;
        let area = buf.area;
        let same_size = self.shown.as_ref().is_some_and(|s| s.area == area);
        // in usize: small text on a big screen is more than 65,535 cells
        let width = area.width as usize;
        let mut painted = vec![false; area.height as usize];
        for (y, fresh) in painted.iter_mut().enumerate() {
            let row = y * width..(y + 1) * width;
            if same_size && self.shown.as_ref().unwrap().content[row.clone()] == buf.content[row] {
                continue;
            }
            *fresh = true;
            let py = self.origin.1 + y * ch;
            if py + ch > self.size.1 {
                break;
            }
            let mut x = 0;
            while x < width {
                let cell = &buf.content[y * width + x];
                let cols = cell.symbol().width().clamp(1, 2).min(width - x);
                let px = self.origin.0 + x * cw;
                if px + cw * cols > stride {
                    break; // a window narrower than one cell (minimized)
                }
                paint_cell(
                    &mut self.glyphs,
                    &mut self.pixels,
                    stride,
                    px,
                    py,
                    cell,
                    cols,
                );
                x += cols;
            }
        }
        // zoomed-in map tiles nothing was drawn over: one glyph stretched over each
        for b in app.map_blocks.borrow().iter() {
            let r = b.rect;
            let (x, y) = (r.x as usize, r.y as usize);
            let (px, py) = (self.origin.0 + x * cw, self.origin.1 + y * ch);
            if !(y..y + r.height as usize).any(|y| painted.get(y) == Some(&true))
                || px + cw * r.width as usize > stride
                || py + ch * r.height as usize > self.size.1
            {
                continue;
            }
            let same = (0..r.height)
                .flat_map(|dy| (0..r.width).map(move |dx| (dx, dy)))
                .zip(&b.cells)
                .all(|((dx, dy), c)| buf.cell((r.x + dx, r.y + dy)) == Some(c));
            if same {
                let glyph = &b.cells[(b.glyph.1 * r.width + b.glyph.0) as usize];
                paint_block(
                    &mut self.glyphs,
                    &mut self.pixels,
                    stride,
                    px,
                    py,
                    glyph,
                    r.width as usize,
                    r.height as usize,
                );
            }
        }
        self.shown = Some(buf.clone());
        if let Some(surface) = self.surface.as_mut() {
            let mut out = surface
                .buffer_mut()
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            if out.len() == self.pixels.len() {
                out.copy_from_slice(&self.pixels);
            }
            out.present()
                .map_err(|e| std::io::Error::other(e.to_string()))?;
        }
        Ok(())
    }
}

/// The text size to start with, from settings (0 = normal). Sizes under the
/// smallest step are left over from the old pixel-at-a-time zoom, which could
/// go that small by accident: start at the normal size instead.
fn remembered_size(px: f32) -> f32 {
    if px < FONT_MIN { 0.0 } else { px.min(FONT_MAX) }
}

/// The next zoom step from `now` in direction `dir` (0: the normal size),
/// or None when there is no further step that way.
fn zoom_step(now: f32, dir: i32) -> Option<f32> {
    match dir {
        0 => Some(FONT_DEFAULT),
        d if d > 0 => ZOOM_STEPS.iter().copied().find(|s| *s > now + 0.01),
        _ => ZOOM_STEPS.iter().rev().copied().find(|s| *s < now - 0.01),
    }
}

fn paint_cell(
    glyphs: &mut Glyphs,
    pixels: &mut [u32],
    stride: usize,
    px: usize,
    py: usize,
    cell: &Cell,
    cols: usize,
) {
    paint_block(glyphs, pixels, stride, px, py, cell, cols, 0);
}

/// Paint one cell's glyph and colors over a block `cols` by `rows` cells big,
/// the glyph stretched to fill it (`rows` 0: an ordinary cell, `cols` wide).
#[allow(clippy::too_many_arguments)]
fn paint_block(
    glyphs: &mut Glyphs,
    pixels: &mut [u32],
    stride: usize,
    px: usize,
    py: usize,
    cell: &Cell,
    cols: usize,
    rows: usize,
) {
    let (cw, ch) = (glyphs.cw * cols, glyphs.ch * rows.max(1));
    let m = cell.modifier;
    let mut fg = rgb(cell.fg, DEFAULT_FG);
    let mut bg = rgb(cell.bg, DEFAULT_BG);
    if m.contains(Modifier::REVERSED) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if m.contains(Modifier::DIM) {
        fg = mix(bg, fg, 140);
    }
    if m.contains(Modifier::HIDDEN) {
        fg = bg;
    }
    let c = cell.symbol().chars().next().unwrap_or(' ');
    let underline = m
        .contains(Modifier::UNDERLINED)
        .then(|| glyphs.underline_row());
    let strike = m.contains(Modifier::CROSSED_OUT).then_some(ch / 2);
    let bold = m.contains(Modifier::BOLD);
    let cov = if rows == 0 {
        glyphs.get(c, bold, cols)
    } else {
        glyphs.get_block(c, bold, cols, rows)
    };
    for y in 0..ch {
        let line = underline == Some(y) || strike == Some(y);
        let out = &mut pixels[(py + y) * stride + px..(py + y) * stride + px + cw];
        let src = &cov[y * cw..(y + 1) * cw];
        for (o, &a) in out.iter_mut().zip(src) {
            let a = if line { 255 } else { a };
            *o = pack(match a {
                0 => bg,
                255 => fg,
                a => mix(bg, fg, a),
            });
        }
    }
}

fn pack((r, g, b): (u8, u8, u8)) -> u32 {
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: u8) -> (u8, u8, u8) {
    let f = |x: u8, y: u8| ((x as u32 * (255 - t as u32) + y as u32 * t as u32) / 255) as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

/// A cell color as RGB. Named colors use a soft, readable terminal scheme.
fn rgb(c: Color, default: (u8, u8, u8)) -> (u8, u8, u8) {
    const ANSI: [(u8, u8, u8); 16] = [
        (20, 20, 24),
        (205, 60, 60),
        (90, 180, 80),
        (215, 175, 60),
        (70, 120, 210),
        (170, 90, 190),
        (70, 170, 200),
        (200, 200, 195),
        (110, 110, 115),
        (240, 100, 100),
        (130, 220, 120),
        (245, 225, 120),
        (110, 160, 250),
        (210, 130, 230),
        (110, 215, 230),
        (245, 245, 240),
    ];
    match c {
        Color::Reset => default,
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => ANSI[0],
        Color::Red => ANSI[1],
        Color::Green => ANSI[2],
        Color::Yellow => ANSI[3],
        Color::Blue => ANSI[4],
        Color::Magenta => ANSI[5],
        Color::Cyan => ANSI[6],
        Color::Gray => ANSI[7],
        Color::DarkGray => ANSI[8],
        Color::LightRed => ANSI[9],
        Color::LightGreen => ANSI[10],
        Color::LightYellow => ANSI[11],
        Color::LightBlue => ANSI[12],
        Color::LightMagenta => ANSI[13],
        Color::LightCyan => ANSI[14],
        Color::White => ANSI[15],
        Color::Indexed(i) => match i {
            0..=15 => ANSI[i as usize],
            16..=231 => {
                let i = i - 16;
                let v = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
                (v(i / 36), v(i / 6 % 6), v(i % 6))
            }
            _ => {
                let g = 8 + (i - 232) * 10;
                (g, g, g)
            }
        },
    }
}

fn modifiers(m: ModifiersState) -> KeyModifiers {
    let mut k = KeyModifiers::NONE;
    if m.shift_key() {
        k |= KeyModifiers::SHIFT;
    }
    if m.control_key() {
        k |= KeyModifiers::CONTROL;
    }
    if m.alt_key() {
        k |= KeyModifiers::ALT;
    }
    k
}

/// A window key press as the crossterm event a terminal would have sent.
/// Held keys repeat as presses, as they do in a terminal.
fn key_event(e: &winit::event::KeyEvent, mods: ModifiersState) -> Option<KeyEvent> {
    let mut modifiers = modifiers(mods);
    let code = match &e.logical_key {
        Key::Named(n) => match n {
            NamedKey::Enter => KeyCode::Enter,
            NamedKey::Escape => KeyCode::Esc,
            NamedKey::Backspace => KeyCode::Backspace,
            NamedKey::Tab if mods.shift_key() => KeyCode::BackTab,
            NamedKey::Tab => KeyCode::Tab,
            NamedKey::Space => KeyCode::Char(' '),
            NamedKey::ArrowLeft => KeyCode::Left,
            NamedKey::ArrowRight => KeyCode::Right,
            NamedKey::ArrowUp => KeyCode::Up,
            NamedKey::ArrowDown => KeyCode::Down,
            NamedKey::Home => KeyCode::Home,
            NamedKey::End => KeyCode::End,
            NamedKey::PageUp => KeyCode::PageUp,
            NamedKey::PageDown => KeyCode::PageDown,
            NamedKey::Delete => KeyCode::Delete,
            NamedKey::Insert => KeyCode::Insert,
            NamedKey::F1 => KeyCode::F(1),
            NamedKey::F2 => KeyCode::F(2),
            NamedKey::F3 => KeyCode::F(3),
            NamedKey::F4 => KeyCode::F(4),
            NamedKey::F5 => KeyCode::F(5),
            NamedKey::F6 => KeyCode::F(6),
            NamedKey::F7 => KeyCode::F(7),
            NamedKey::F8 => KeyCode::F(8),
            NamedKey::F9 => KeyCode::F(9),
            NamedKey::F10 => KeyCode::F(10),
            NamedKey::F12 => KeyCode::F(12),
            _ => return None,
        },
        Key::Character(s) => {
            // Ctrl+letter: the plain letter, as terminals report it
            let s = if mods.control_key() || mods.alt_key() {
                match e.key_without_modifiers() {
                    Key::Character(base) => base.to_string(),
                    _ => s.to_string(),
                }
            } else {
                s.to_string()
            };
            let c = s.chars().next()?;
            if c.is_control() {
                return None;
            }
            // shift is in the character itself (A, ?), as in a terminal
            if !c.is_alphabetic() {
                modifiers.remove(KeyModifiers::SHIFT);
            }
            KeyCode::Char(c)
        }
        // No character came with the key. Wine does this for many letters
        // (winit takes a letter's character from the WM_CHAR message that
        // should follow the key press), so work it out from the key itself.
        _ => {
            let c = fallback_char(e, mods.shift_key())?;
            if !c.is_alphabetic() {
                modifiers.remove(KeyModifiers::SHIFT);
            }
            KeyCode::Char(c)
        }
    };
    Some(KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

/// The character a key types when the system didn't say: the layout's own
/// character if winit knows it, else the key's place on a US keyboard.
fn fallback_char(e: &winit::event::KeyEvent, shift: bool) -> Option<char> {
    if let Key::Character(base) = e.key_without_modifiers() {
        let c = base.chars().next()?;
        if !c.is_control() {
            return Some(if shift { c.to_uppercase().next()? } else { c });
        }
    }
    let PhysicalKey::Code(code) = e.physical_key else {
        return None;
    };
    us_char(code, shift)
}

/// What a key types on a US layout.
fn us_char(code: PhysCode, shift: bool) -> Option<char> {
    use PhysCode as K;
    const LETTERS: [K; 26] = [
        K::KeyA,
        K::KeyB,
        K::KeyC,
        K::KeyD,
        K::KeyE,
        K::KeyF,
        K::KeyG,
        K::KeyH,
        K::KeyI,
        K::KeyJ,
        K::KeyK,
        K::KeyL,
        K::KeyM,
        K::KeyN,
        K::KeyO,
        K::KeyP,
        K::KeyQ,
        K::KeyR,
        K::KeyS,
        K::KeyT,
        K::KeyU,
        K::KeyV,
        K::KeyW,
        K::KeyX,
        K::KeyY,
        K::KeyZ,
    ];
    const DIGITS: [K; 10] = [
        K::Digit0,
        K::Digit1,
        K::Digit2,
        K::Digit3,
        K::Digit4,
        K::Digit5,
        K::Digit6,
        K::Digit7,
        K::Digit8,
        K::Digit9,
    ];
    if let Some(i) = LETTERS.iter().position(|k| *k == code) {
        let c = (b'a' + i as u8) as char;
        return Some(if shift { c.to_ascii_uppercase() } else { c });
    }
    if let Some(i) = DIGITS.iter().position(|k| *k == code) {
        return Some(if shift {
            b")!@#$%^&*("[i] as char
        } else {
            (b'0' + i as u8) as char
        });
    }
    let (plain, shifted) = match code {
        K::Minus => ('-', '_'),
        K::Equal => ('=', '+'),
        K::BracketLeft => ('[', '{'),
        K::BracketRight => (']', '}'),
        K::Backslash => ('\\', '|'),
        K::Semicolon => (';', ':'),
        K::Quote => ('\'', '"'),
        K::Comma => (',', '<'),
        K::Period => ('.', '>'),
        K::Slash => ('/', '?'),
        K::Backquote => ('`', '~'),
        K::NumpadAdd => ('+', '+'),
        K::NumpadSubtract => ('-', '-'),
        _ => return None,
    };
    Some(if shift { shifted } else { plain })
}

fn icon() -> Option<Icon> {
    let mut r = png::Decoder::new(std::io::Cursor::new(ICON_PNG))
        .read_info()
        .ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    (info.color_type == png::ColorType::Rgba).then_some(())?;
    Icon::from_rgba(buf, info.width, info.height).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_decodes() {
        assert!(icon().is_some());
    }

    #[test]
    fn colors_map_like_a_terminal() {
        assert_eq!(rgb(Color::Reset, DEFAULT_BG), DEFAULT_BG);
        assert_eq!(rgb(Color::Rgb(1, 2, 3), DEFAULT_FG), (1, 2, 3));
        assert_eq!(rgb(Color::Indexed(16), DEFAULT_FG), (0, 0, 0));
        assert_eq!(rgb(Color::Indexed(231), DEFAULT_FG), (255, 255, 255));
        assert_eq!(mix((0, 0, 0), (255, 255, 255), 255), (255, 255, 255));
    }

    #[test]
    fn keys_without_text_still_type() {
        assert_eq!(us_char(PhysCode::KeyJ, false), Some('j'));
        assert_eq!(us_char(PhysCode::KeyJ, true), Some('J'));
        assert_eq!(us_char(PhysCode::Digit1, false), Some('1'));
        assert_eq!(us_char(PhysCode::Slash, true), Some('?'));
        assert_eq!(us_char(PhysCode::Equal, true), Some('+'));
        assert_eq!(us_char(PhysCode::ShiftLeft, false), None);
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::*;

    #[test]
    fn zoom_steps_up_down_and_stops() {
        assert_eq!(zoom_step(16.0, 1), Some(18.0));
        assert_eq!(zoom_step(16.0, -1), Some(15.0));
        assert_eq!(
            zoom_step(17.0, -1),
            Some(16.0),
            "an old in-between size joins the steps"
        );
        assert_eq!(
            zoom_step(FONT_MIN, -1),
            None,
            "never smaller than the smallest step"
        );
        assert_eq!(zoom_step(FONT_MAX, 1), None);
        assert_eq!(zoom_step(9.0, 1), Some(10.0));
        assert_eq!(zoom_step(30.0, 0), Some(FONT_DEFAULT));
        assert_eq!(
            remembered_size(9.0),
            0.0,
            "a leftover tiny size starts normal"
        );
        assert_eq!(remembered_size(0.0), 0.0);
        assert_eq!(remembered_size(12.0), 12.0);
        assert_eq!(remembered_size(90.0), FONT_MAX);
    }
}

#[cfg(test)]
mod paint_tests {
    use super::*;
    use crate::world::Settings;

    fn win(px: f32) -> Win {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_win_paint.json"),
        );
        app.font_px = px;
        Win {
            app,
            window: None,
            surface: None,
            glyphs: Glyphs::new(px),
            term: Terminal::new(TestBackend::new(80, 24)).unwrap(),
            clock: Clock::new(),
            pixels: vec![],
            size: (0, 0),
            grid: (0, 0),
            origin: (0, 0),
            shown: None,
            mods: ModifiersState::empty(),
            held: None,
            wheel: 0.0,
            scale: 1.0,
            next_frame: Instant::now(),
            failed: false,
            error: None,
        }
    }

    /// Small text on a big screen is more than 65,535 cells; a minimized
    /// window is smaller than one cell. Neither may crash the painter.
    #[test]
    fn any_window_size_paints() {
        let mut w = win(FONT_MIN);
        for (x, y) in [
            (3840, 2160),
            (2560, 1440),
            (0, 0),
            (1, 1),
            (5, 300),
            (1280, 720),
        ] {
            w.resize(PhysicalSize::new(x, y));
            w.paint().unwrap();
            w.paint().unwrap(); // the second frame takes the "only changed rows" path
        }
        w.app.screen = Screen::Game;
        w.app.world = Some(crate::world::World::new(42, Settings::default()));
        w.resize(PhysicalSize::new(3840, 2160));
        assert!(
            w.grid.0 as usize * w.grid.1 as usize > u16::MAX as usize,
            "the big case is really big"
        );
        w.paint().unwrap();
        w.paint().unwrap();
    }

    /// Zoomed in, the map's tiles are painted as blocks, every frame alike.
    #[test]
    fn zoomed_map_paints_blocks() {
        for zoom in [
            crate::ui::view::MapZoom::Close,
            crate::ui::view::MapZoom::Closer,
        ] {
            let mut w = win(16.0);
            w.app.screen = Screen::Game;
            w.app.world = Some(crate::world::World::new(42, Settings::default()));
            w.app.map_zoom = zoom;
            w.app.center_view = true;
            w.resize(PhysicalSize::new(1600, 900));
            w.paint().unwrap();
            assert!(
                !w.app.map_blocks.borrow().is_empty(),
                "{zoom:?} notes its blocks"
            );
            let first = w.pixels.clone();
            w.shown = None;
            w.paint().unwrap();
            assert!(
                w.pixels == first,
                "a full repaint matches the row-by-row one"
            );
            if let Ok(dir) = std::env::var("RAILHEAD_PPM") {
                let mut out = format!("P6 {} {} 255\n", w.size.0, w.size.1).into_bytes();
                for p in &w.pixels {
                    out.extend([(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
                }
                std::fs::write(format!("{dir}/zoom_{zoom:?}.ppm"), out).unwrap();
            }
        }
    }
}
