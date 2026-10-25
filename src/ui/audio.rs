//! Music and sound effects, synthesized on the fly. No asset files.
//!
//! The sound of the steam railway: a reedy harmonica lead, plucked guitar and
//! banjo (Karplus–Strong strings), an upright bass, the clickety-clack of
//! wheels over rail joints, a brushed "chuff" and a three-chime steam whistle.
//!
//! Three pieces, written below as plain note lists:
//! - **Departure** (title screen): the engine chuffs faster as it pulls out, a whistle, then a slow tune.
//! - **Main Line** (in game): a swung folk train-beat in G.
//! - **Night Freight** (in game, alternating with Main Line): an E-minor ballad, distant whistles.
//!
//! The music follows the game (`Mood`): paused, it drops its percussion and
//! gets quieter, like a train idling at the platform.
//!
//! The UI calls `Audio::play` / `Audio::mood`; commands go down a channel to
//! `Synth`, which the sound card pulls samples from on its own thread.
//! Everything works (silently) without a sound card or the `audio` feature.
//! Mixing levels live in `balance.rs`; the notes and tempos are the
//! composition, so they live here.

use std::{
    f32::consts::TAU,
    sync::mpsc::{Receiver, Sender, channel},
};

use crate::balance as B;

const RATE: u32 = 44_100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx {
    /// Something got built or changed: a hammer on a rail spike.
    Click,
    /// The thing you tried didn't work.
    Error,
    /// A new train: the steam whistle.
    Horn,
    /// A new truck.
    Beep,
    /// Money from a delivery: the station bell.
    Coin,
    /// A good alert: a town grew, a contract was filled.
    Good,
    Warn,
    Danger,
    /// A tutorial step done, the Charter finished.
    Fanfare,
    /// The Railhead Gazette: a telegraph tapping out the news, then a bell.
    News,
    /// A Charter medal: the fanfare, bigger, with bells.
    Medal,
}

pub const ALL_SFX: [Sfx; 11] = [
    Sfx::Click,
    Sfx::Error,
    Sfx::Horn,
    Sfx::Beep,
    Sfx::Coin,
    Sfx::Good,
    Sfx::Warn,
    Sfx::Danger,
    Sfx::Fanfare,
    Sfx::News,
    Sfx::Medal,
];

/// What the game is doing, so the music can fit it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Title,
    Running,
    Paused,
}

/// How loud things are: music and effects in percent (0..=100), and a mute
/// switch that keeps the levels for when the sound comes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Levels {
    pub music: u8,
    pub sfx: u8,
    pub muted: bool,
}

impl Default for Levels {
    fn default() -> Levels {
        Levels {
            music: B::MUSIC_PERCENT_DEFAULT,
            sfx: B::SFX_PERCENT_DEFAULT,
            muted: false,
        }
    }
}

impl Levels {
    /// The mixer's music and effects gains for these levels (0 when muted).
    fn gains(self) -> (f32, f32) {
        if self.muted {
            return (0.0, 0.0);
        }
        (
            self.music as f32 / 100.0 * B::MUSIC_VOLUME,
            self.sfx as f32 / 100.0 * B::SFX_VOLUME,
        )
    }

    /// "Sound: music 70% · effects 70%." or "Sound: muted."
    pub fn label(self) -> String {
        if self.muted {
            "Sound: muted.".into()
        } else {
            format!("Sound: music {}% · effects {}%.", self.music, self.sfx)
        }
    }
}

/// A volume slider one step up or down, kept within 0..=100.
pub fn step_percent(v: u8, up: bool) -> u8 {
    if up {
        v.saturating_add(B::VOLUME_STEP).min(100)
    } else {
        v.saturating_sub(B::VOLUME_STEP)
    }
}

enum Cmd {
    Sfx(Sfx),
    /// Music on or off, and the music and effects gains.
    Levels {
        music_on: bool,
        music: f32,
        sfx: f32,
    },
    Mood(Mood),
}

pub struct Audio {
    /// Change it with `set_levels`, so the mixer hears about it.
    pub levels: Levels,
    mood: Option<Mood>,
    tx: Option<Sender<Cmd>>,
    #[cfg(feature = "audio")]
    _sink: Option<rodio::MixerDeviceSink>,
}

impl Audio {
    /// No sound at all (tests, the headless harness, snapshots).
    pub fn silent() -> Audio {
        Audio {
            levels: Levels::default(),
            mood: None,
            tx: None,
            #[cfg(feature = "audio")]
            _sink: None,
        }
    }

    /// Open the default sound device. If there isn't one, stay silent.
    #[cfg(feature = "audio")]
    pub fn open(levels: Levels) -> Audio {
        let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else {
            return Audio {
                levels,
                ..Audio::silent()
            };
        };
        sink.log_on_drop(false);
        let (tx, rx) = channel();
        sink.mixer().add(Synth::new(rx));
        let mut audio = Audio {
            levels,
            tx: Some(tx),
            _sink: Some(sink),
            mood: None,
        };
        audio.set_levels(levels);
        audio
    }

    #[cfg(not(feature = "audio"))]
    pub fn open(levels: Levels) -> Audio {
        Audio {
            levels,
            ..Audio::silent()
        }
    }

    fn send(&self, c: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(c);
        }
    }

    pub fn play(&self, s: Sfx) {
        if !self.levels.muted && self.levels.sfx > 0 {
            self.send(Cmd::Sfx(s));
        }
    }

    /// Tell the music what's happening. Cheap to call every frame.
    pub fn mood(&mut self, m: Mood) {
        if self.mood != Some(m) {
            self.mood = Some(m);
            self.send(Cmd::Mood(m));
        }
    }

    /// True if a sound device is open.
    pub fn live(&self) -> bool {
        self.tx.is_some()
    }

    /// `railhead --sound-test`: every effect, then a little of each piece.
    pub fn demo() -> bool {
        let mut audio = Audio::open(Levels {
            music: 0,
            ..Levels::default()
        });
        if !audio.live() {
            return false;
        }
        let pause = |ms| std::thread::sleep(std::time::Duration::from_millis(ms));
        for s in ALL_SFX {
            println!("  {s:?}");
            audio.play(s);
            pause(1000);
        }
        audio.set_levels(Levels::default());
        for (name, mood) in [
            ("Departure (title)", Mood::Title),
            ("Main Line (in game)", Mood::Running),
            ("...paused", Mood::Paused),
        ] {
            println!("  {name}");
            audio.mood(mood);
            pause(14_000);
        }
        true
    }

    pub fn set_levels(&mut self, l: Levels) {
        self.levels = l;
        let (music, sfx) = l.gains();
        self.send(Cmd::Levels {
            music_on: music > 0.0,
            music,
            sfx,
        });
    }

    /// Mute everything, or bring the sound back as it was.
    pub fn toggle_mute(&mut self) {
        self.set_levels(Levels {
            muted: !self.levels.muted,
            ..self.levels
        });
    }
}

// ------------------------------------------------------------------ instruments

#[derive(Clone, Copy, Debug, PartialEq)]
enum Inst {
    /// Plain chip voices with a straight attack/decay (UI blips).
    Square,
    Pulse,
    /// Harmonica-ish lead: a soft pulse, filtered, with a slow vibrato.
    Reed,
    /// Plucked string (guitar, banjo): Karplus–Strong.
    Pluck,
    /// A brighter, quicker-dying pluck.
    Banjo,
    /// Upright bass: a round triangle that dies away.
    Bass,
    /// A wheel crossing a rail joint.
    Clack,
    /// A brushed snare / the "chuff" of exhaust steam.
    Brush,
    /// One chime of a steam whistle (play three for the chord).
    Whistle,
    /// A station bell.
    Bell,
    /// A soft organ-like pad for long chords.
    Pad,
}

impl Inst {
    fn is_drum(self) -> bool {
        matches!(self, Inst::Clack | Inst::Brush)
    }
}

struct Voice {
    inst: Inst,
    freq: f32,
    /// Frequency multiplier per second (1.0 = steady; >1 slides up).
    slide: f32,
    phase: f32,
    delay: u32,
    len: u32,
    age: u32,
    vol: f32,
    /// One-pole low-pass filter memory.
    lp: f32,
    /// Karplus–Strong delay line (plucked strings only).
    ks: Vec<f32>,
    ks_i: usize,
}

fn midi(n: f32) -> f32 {
    440.0 * 2f32.powf((n - 69.0) / 12.0)
}

fn ms(t: f32) -> u32 {
    (t * RATE as f32 / 1000.0) as u32
}

fn voice(at: f32, inst: Inst, n: f32, dur: f32, vol: f32) -> Voice {
    Voice {
        inst,
        freq: midi(n),
        slide: 1.0,
        phase: 0.0,
        delay: ms(at),
        len: ms(dur).max(1),
        age: 0,
        vol,
        lp: 0.0,
        ks: vec![],
        ks_i: 0,
    }
}

/// A classic chip note `at` ms from now, `dur` ms long.
fn note(at: f32, n: f32, dur: f32, inst: Inst, vol: f32) -> Voice {
    voice(at, inst, n, dur, vol)
}

fn noise(state: &mut u32) -> f32 {
    // xorshift32
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state as f32 / u32::MAX as f32) * 2.0 - 1.0
}

fn chip(inst: Inst, phase: f32, n: &mut u32) -> f32 {
    match inst {
        Inst::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Inst::Pulse => {
            if phase < 0.25 {
                1.0
            } else {
                -1.0
            }
        }
        _ => noise(n),
    }
}

impl Voice {
    /// Next sample, or None when the note is over.
    fn next(&mut self, n: &mut u32) -> Option<f32> {
        if self.delay > 0 {
            self.delay -= 1;
            return Some(0.0);
        }
        if self.age >= self.len {
            return None;
        }
        let t = self.age as f32 / RATE as f32; // seconds since the note started
        let left = (self.len - self.age) as f32 / RATE as f32; // seconds until it ends
        let release = |secs: f32| (left / secs).min(1.0);
        let attack = |secs: f32| (t / secs).min(1.0);
        let step = |v: &mut Voice, f: f32| v.phase = (v.phase + f / RATE as f32).fract();
        let s = match self.inst {
            Inst::Square | Inst::Pulse => {
                let env = attack(0.004) * (1.0 - self.age as f32 / self.len as f32);
                let s = chip(self.inst, self.phase, n) * env;
                let f = self.freq;
                step(self, f);
                s
            }
            Inst::Reed => {
                let vib = 1.0 + 0.006 * (TAU * 5.5 * t).sin() * ((t - 0.15) / 0.2).clamp(0.0, 1.0);
                let raw = if self.phase < 0.32 { 1.0 } else { -1.0 };
                self.lp += (raw - self.lp) * 0.22;
                let breath = noise(n) * 0.04;
                let f = self.freq * vib;
                step(self, f);
                (self.lp + breath) * attack(0.03) * release(0.07) * (0.85 + 0.15 * (-t * 2.0).exp())
            }
            Inst::Pluck | Inst::Banjo => {
                if self.ks.is_empty() {
                    let len = (RATE as f32 / self.freq).max(2.0) as usize;
                    let mut prev = 0.0;
                    self.ks = (0..len)
                        .map(|_| {
                            // a guitar starts mellow (filtered noise), a banjo bright
                            let x = noise(n);
                            prev += (x - prev) * if self.inst == Inst::Banjo { 0.9 } else { 0.55 };
                            prev
                        })
                        .collect();
                }
                let keep = if self.inst == Inst::Banjo {
                    0.990
                } else {
                    0.996
                };
                let i = self.ks_i;
                let j = (i + 1) % self.ks.len();
                let out = self.ks[i];
                self.ks[i] = (self.ks[i] + self.ks[j]) * 0.5 * keep;
                self.ks_i = j;
                out * release(0.04)
            }
            Inst::Bass => {
                let raw = 4.0 * (self.phase - 0.5).abs() - 1.0;
                self.lp += (raw - self.lp) * 0.5;
                let f = self.freq;
                step(self, f);
                self.lp * attack(0.006) * (0.35 + 0.65 * (-t / 0.25).exp()) * release(0.04)
            }
            Inst::Clack => {
                let x = noise(n);
                self.lp += (x - self.lp) * 0.35;
                let hiss = (x - self.lp) * (-t / 0.008).exp();
                let ping = (TAU * 2300.0 * t).sin() * (-t / 0.005).exp() * 0.5;
                (hiss + ping) * release(0.004)
            }
            Inst::Brush => {
                let x = noise(n);
                self.lp += (x - self.lp) * 0.3;
                self.lp * attack(0.003) * (-t / 0.045).exp() * 1.6
            }
            Inst::Whistle => {
                // the pitch rises as the steam valve opens and sags as it shuts
                let bend = 1.0 - 0.06 * (-t / 0.07).exp() - 0.03 * (1.0 - (left / 0.15).min(1.0));
                let vib = 1.0 + 0.003 * (TAU * 6.0 * t).sin();
                let raw = 4.0 * (self.phase - 0.5).abs() - 1.0;
                self.lp += (raw - self.lp) * 0.4;
                let f = self.freq * bend * vib;
                step(self, f);
                (self.lp + noise(n) * 0.12) * attack(0.06) * release(0.18)
            }
            Inst::Bell => {
                let a = (TAU * self.freq * t).sin();
                let b = (TAU * self.freq * 2.76 * t).sin() * 0.45;
                let c = (TAU * self.freq * 5.4 * t).sin() * 0.2 * (-t / 0.05).exp();
                (a + b + c) * (-t / 0.35).exp() * release(0.02)
            }
            Inst::Pad => {
                let a = 4.0 * (self.phase - 0.5).abs() - 1.0;
                let b = (TAU * self.freq * 1.004 * t).sin();
                self.lp += ((a + b) * 0.5 - self.lp) * 0.15;
                let f = self.freq;
                step(self, f);
                self.lp * attack(0.35) * release(0.5)
            }
        };
        if self.slide != 1.0 {
            self.freq *= self.slide.powf(1.0 / RATE as f32);
        }
        self.age += 1;
        Some(s * self.vol)
    }
}

/// A steam whistle: three chimes at once (a plaintive minor chord).
fn whistle(at: f32, dur: f32, vol: f32) -> Vec<Voice> {
    [66.0, 69.0, 73.0]
        .iter()
        .map(|n| voice(at, Inst::Whistle, *n, dur, vol))
        .collect()
}

fn sfx_voices(s: Sfx) -> Vec<Voice> {
    use Inst::*;
    match s {
        Sfx::Click => vec![
            voice(0.0, Clack, 60.0, 30.0, 0.8),
            note(0.0, 84.0, 22.0, Pulse, 0.25),
            voice(8.0, Bell, 96.0, 90.0, 0.12),
        ],
        Sfx::Error => vec![
            note(0.0, 45.0, 90.0, Square, 0.45),
            note(100.0, 41.0, 140.0, Square, 0.45),
        ],
        Sfx::Horn => whistle(0.0, 520.0, 0.45),
        Sfx::Beep => vec![
            note(0.0, 76.0, 70.0, Pulse, 0.45),
            note(90.0, 76.0, 70.0, Pulse, 0.45),
        ],
        Sfx::Coin => vec![
            voice(0.0, Bell, 88.0, 350.0, 0.35),
            voice(90.0, Bell, 93.0, 450.0, 0.3),
        ],
        Sfx::Good => vec![
            voice(0.0, Pluck, 72.0, 400.0, 0.7),
            voice(80.0, Pluck, 76.0, 400.0, 0.7),
            voice(160.0, Pluck, 79.0, 600.0, 0.7),
            voice(160.0, Bell, 91.0, 400.0, 0.15),
        ],
        Sfx::Warn => vec![
            note(0.0, 69.0, 120.0, Square, 0.35),
            note(140.0, 65.0, 180.0, Square, 0.35),
        ],
        Sfx::Danger => (0..4)
            .map(|i| {
                voice(
                    i as f32 * 140.0,
                    Bell,
                    if i % 2 == 0 { 81.0 } else { 76.0 },
                    160.0,
                    0.45,
                )
            })
            .collect(),
        Sfx::News => {
            // ·-·· -·  (a telegraph sounder), then the newsboy's bell
            let mut v = vec![];
            let mut t = 0.0;
            for dah in [false, true, false, false, true, false] {
                let len = if dah { 150.0 } else { 55.0 };
                v.push(note(t, 81.0, len, Pulse, 0.3));
                v.push(voice(t, Clack, 70.0, 20.0, 0.5));
                t += len + 55.0;
            }
            v.push(voice(t + 60.0, Bell, 91.0, 600.0, 0.35));
            v.push(voice(t + 60.0, Bell, 86.0, 600.0, 0.2));
            v
        }
        Sfx::Medal => {
            let mut v = sfx_voices(Sfx::Fanfare);
            for (i, n) in [79.0, 84.0, 88.0, 91.0].iter().enumerate() {
                v.push(voice(560.0 + i as f32 * 110.0, Bell, *n, 900.0, 0.3));
            }
            v.push(voice(560.0, Pluck, 43.0, 1200.0, 0.8));
            v
        }
        Sfx::Fanfare => {
            let mut v = vec![
                voice(0.0, Reed, 67.0, 140.0, 0.45),
                voice(140.0, Reed, 72.0, 140.0, 0.45),
                voice(280.0, Reed, 76.0, 140.0, 0.45),
                voice(420.0, Reed, 79.0, 520.0, 0.45),
                voice(420.0, Pluck, 55.0, 700.0, 0.7),
                voice(420.0, Pluck, 64.0, 700.0, 0.6),
            ];
            v.extend(whistle(950.0, 420.0, 0.25));
            v
        }
    }
}

// ------------------------------------------------------------------ the score

/// One note of a piece: when (in beats from the start of the loop), what, how long, how loud.
#[derive(Clone, Copy, Debug)]
struct Ev {
    beat: f32,
    inst: Inst,
    note: f32,
    beats: f32,
    vol: f32,
}

/// A note name ("F#4", "Bb3") as a MIDI number. Middle C is C4 = 60.
fn pitch(name: &str) -> f32 {
    let b = name.as_bytes();
    let base = match b[0] {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => panic!("bad note {name}"),
    };
    let (acc, rest) = match b.get(1) {
        Some(b'#') => (1, &name[2..]),
        Some(b'b') => (-1, &name[2..]),
        _ => (0, &name[1..]),
    };
    let octave: i32 = rest
        .parse()
        .unwrap_or_else(|_| panic!("bad octave in {name}"));
    (12 * (octave + 1) + base + acc) as f32
}

/// "B4:.5 D5:.5 r:1" → notes for `inst` starting at `at` beats. Returns the length in beats.
fn melody(out: &mut Vec<Ev>, at: f32, inst: Inst, vol: f32, transpose: f32, tune: &str) -> f32 {
    let mut t = at;
    for tok in tune.split_whitespace() {
        let (n, d) = tok
            .split_once(':')
            .unwrap_or_else(|| panic!("bad token {tok}"));
        let beats: f32 = d.parse().unwrap_or_else(|_| panic!("bad length in {tok}"));
        if n != "r" {
            out.push(Ev {
                beat: t,
                inst,
                note: pitch(n) + transpose,
                beats: beats * 0.92,
                vol,
            });
        }
        t += beats;
    }
    t - at
}

/// A chord symbol ("G", "Em", "A7", "F#m") as (root pitch class, intervals).
fn chord(sym: &str) -> (f32, &'static [f32]) {
    let (root, rest) = if sym.len() > 1 && (sym.as_bytes()[1] == b'#' || sym.as_bytes()[1] == b'b')
    {
        sym.split_at(2)
    } else {
        sym.split_at(1)
    };
    let pc = pitch(&format!("{root}0")) - 12.0;
    let iv: &'static [f32] = match rest {
        "" => &[0.0, 4.0, 7.0],
        "m" => &[0.0, 3.0, 7.0],
        "7" => &[0.0, 4.0, 7.0, 10.0],
        "m7" => &[0.0, 3.0, 7.0, 10.0],
        _ => panic!("bad chord {sym}"),
    };
    (pc, iv)
}

/// The chord tone nearest above `low` for each interval.
fn voicing(sym: &str, low: f32) -> Vec<f32> {
    let (pc, iv) = chord(sym);
    iv.iter()
        .map(|i| {
            let mut n = pc + i;
            while n < low {
                n += 12.0;
            }
            n
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Piece {
    /// The title screen: a train pulling out.
    Departure,
    /// In game, these take turns (`Piece::next`), two times round each.
    /// A boom-chicka country tune with a harmonica.
    MainLine,
    /// A 12-bar blues shuffle, harmonica and walking bass.
    CoalTrainBlues,
    /// A fast reel on the banjo, wheels clattering on every beat.
    Express,
    /// A waltz in three.
    ValleyWaltz,
    /// Quiet: banjo rolls and a far-off whistle.
    NightFreight,
    /// Slow and calm: a guitar over an organ pad, by the water.
    HarbourLights,
}

impl Piece {
    const ALL: [Piece; 7] = [
        Piece::Departure,
        Piece::MainLine,
        Piece::CoalTrainBlues,
        Piece::Express,
        Piece::ValleyWaltz,
        Piece::NightFreight,
        Piece::HarbourLights,
    ];

    fn file_name(self) -> &'static str {
        match self {
            Piece::Departure => "departure",
            Piece::MainLine => "main-line",
            Piece::CoalTrainBlues => "coal-train-blues",
            Piece::Express => "express",
            Piece::ValleyWaltz => "valley-waltz",
            Piece::NightFreight => "night-freight",
            Piece::HarbourLights => "harbour-lights",
        }
    }
}

struct Score {
    bpm: f32,
    /// Played once, before the loop (beats long).
    intro: Vec<Ev>,
    intro_beats: f32,
    body: Vec<Ev>,
    beats: f32,
}

const MAIN_CHORDS: [&str; 32] = [
    "G", "G", "C", "G", "G", "Em", "A7", "D7", // A
    "G", "G", "C", "G", "C", "G", "D7", "G", // A'
    "C", "C", "G", "G", "A7", "A7", "D", "D7", // B
    "G", "G", "C", "G", "C", "G", "D7", "G", // A'
];
const MAIN_A: &str = "B4:.5 D5:.5 G5:1 F#5:.5 E5:.5 D5:1  B4:1 G4:.5 A4:.5 B4:2  C5:.5 E5:.5 G5:1 E5:.5 C5:.5 E5:1  D5:3 r:1
    B4:.5 D5:.5 G5:1 F#5:.5 G5:.5 A5:1  B5:1.5 G5:.5 E5:2  C#5:.5 E5:.5 A5:1 G5:.5 E5:.5 C#5:1  D5:2 r:.5 A4:.5 C5:.5 F#5:.5";
const MAIN_A2: &str = "G5:1 F#5:.5 E5:.5 D5:1 B4:1  D5:.5 B4:.5 G4:1 A4:.5 B4:.5 D5:1  E5:1 G5:1 E5:.5 D5:.5 C5:1  B4:1 D5:1 G5:2
    E5:.5 F#5:.5 G5:1 E5:1 C5:1  D5:1.5 B4:.5 G4:2  A4:.5 B4:.5 C5:.5 D5:.5 F#5:1 A5:1  G5:3 r:1";
const MAIN_B: &str = "E5:2 G5:2  C6:3 B5:.5 A5:.5  G5:2 D5:2  B4:3 r:1  A4:.5 C#5:.5 E5:.5 G5:.5 A5:2  G5:1 E5:1 C#5:2
    D5:1 F#5:1 A5:2  C6:1 A5:1 F#5:.5 D5:.5 C5:1";

const NIGHT_CHORDS: [&str; 16] = [
    "Em", "Em", "C", "D", "Em", "Em", "Am", "B7", "C", "D", "G", "Em", "Am", "Em", "B7", "Em",
];
const NIGHT_TUNE: &str = "E5:2 G5:1 F#5:1  E5:3 r:1  E5:1 G5:1 C6:1.5 B5:.5  A5:3 r:1  B5:1.5 A5:.5 G5:1 E5:1  G5:1 F#5:1 E5:2
    C5:1 E5:1 A5:1.5 G5:.5  F#5:3 r:1  G5:2 E5:1 G5:1  A5:1.5 F#5:.5 D5:2  B5:2 D6:1 B5:1  G5:3 r:1
    A5:1 C6:1 B5:1 A5:1  G5:1.5 E5:.5 B4:2  D#5:1 F#5:1 A5:1 F#5:1  E5:4";

const DEPART_CHORDS: [&str; 16] = [
    "D", "Bm", "G", "A", "D", "Bm", "G", "A", "G", "A", "Bm", "G", "D", "Bm", "A", "D",
];
const DEPART_TUNE: &str = "F#5:2 A5:1 D6:1  B5:2 A5:1 F#5:1  G5:1.5 F#5:.5 E5:1 D5:1  E5:3 r:1  F#5:2 A5:1 F#5:1  D6:2 C#6:1 B5:1
    B5:1 A5:1 G5:1 E5:1  A5:3 r:1  B5:1.5 A5:.5 G5:2  A5:1 B5:1 C#6:2  D6:1.5 C#6:.5 B5:2  G5:3 r:1
    F#5:2 E5:1 D5:1  B4:2 D5:1 F#5:1  E5:1.5 F#5:.5 E5:1 C#5:1  D5:4";

// Coal Train Blues: two choruses of a 12-bar blues in A, the second one turning home.
const BLUES_CHORDS: [&str; 24] = [
    "A7", "A7", "A7", "A7", "D7", "D7", "A7", "A7", "E7", "D7", "A7", "E7", //
    "A7", "A7", "A7", "A7", "D7", "D7", "A7", "A7", "E7", "D7", "A7", "A7",
];
const BLUES_HEAD: &str =
    "E5:.5 G5:.5 A5:1 G5:.5 E5:.5 C5:1  A4:3 r:1  E5:.5 G5:.5 A5:1 C6:.5 A5:.5 G5:1  E5:3 r:1
    D5:.5 F#5:.5 A5:1 C6:1 A5:1  F#5:2 D5:1 r:1  E5:.5 G5:.5 A5:1 G5:.5 E5:.5 D5:1  C#5:3 r:1
    B4:.5 D5:.5 E5:1 G#5:1 B5:1  A5:.5 F#5:.5 D5:1 C6:1 A5:1  A5:.5 G5:.5 E5:1 C5:.5 C#5:.5 A4:1";
const BLUES_TURN: &str = "B4:2 D5:1 G#4:1";
const BLUES_END: &str = "A4:3 r:1";

// Express: a reel in D, two times through the tune.
const REEL_CHORDS: [&str; 16] = [
    "D", "D", "G", "D", "D", "D", "A7", "A7", "D", "D", "G", "D", "G", "A7", "D", "D",
];
const REEL_TUNE: &str =
    "D5:.5 F#5:.5 A5:.5 F#5:.5 D6:.5 A5:.5 F#5:.5 A5:.5  B5:.5 A5:.5 F#5:.5 A5:.5 D5:.5 F#5:.5 A5:1
    G5:.5 B5:.5 D6:.5 B5:.5 G5:.5 B5:.5 D6:.5 B5:.5  A5:.5 F#5:.5 D5:.5 F#5:.5 A5:2
    D5:.5 E5:.5 F#5:.5 G5:.5 A5:.5 B5:.5 A5:.5 F#5:.5  D6:1 A5:.5 F#5:.5 D5:1 F#5:1
    E5:.5 A5:.5 C#6:.5 A5:.5 E5:.5 A5:.5 C#6:.5 E6:.5  C#6:1 B5:.5 A5:.5 G5:1 E5:1
    D5:.5 F#5:.5 A5:.5 F#5:.5 D6:.5 A5:.5 F#5:.5 A5:.5  B5:.5 A5:.5 F#5:.5 A5:.5 D5:.5 F#5:.5 A5:1
    B5:.5 D6:.5 G6:.5 D6:.5 B5:.5 G5:.5 B5:.5 D6:.5  A5:.5 F#5:.5 A5:.5 D6:.5 F#6:2
    G5:.5 F#5:.5 E5:.5 D5:.5 B4:.5 D5:.5 G5:1  A5:.5 G5:.5 F#5:.5 E5:.5 C#5:.5 E5:.5 A5:1
    F#5:.5 A5:.5 D6:.5 A5:.5 F#5:.5 E5:.5 D5:1  D5:3 r:1";

// Valley Waltz: 32 bars of three in C.
const WALTZ_CHORDS: [&str; 32] = [
    "C", "C", "F", "C", "G", "G7", "C", "C", //
    "C", "C", "F", "F", "C", "G", "C", "C", //
    "F", "F", "C", "C", "G", "G7", "C", "A7", //
    "Dm", "Dm", "C", "Am", "Dm", "G7", "C", "C",
];
const WALTZ_TUNE: &str =
    "E5:2 G5:1  C6:2 G5:1  A5:2 F5:1  E5:3  D5:2 E5:1  F5:1 E5:1 D5:1  E5:2 D5:1  C5:3
    G4:1 C5:1 E5:1  G5:2 E5:1  F5:1 A5:1 C6:1  A5:3  G5:2 E5:1  D5:2 B4:1  C5:2 E5:1  C5:3
    A5:2 C6:1  A5:1 G5:1 F5:1  G5:2 E5:1  C5:3  B4:1 D5:1 G5:1  F5:2 D5:1  E5:2 G5:1  C#5:3
    D5:1 F5:1 A5:1  D6:2 A5:1  G5:2 E5:1  C5:2 A4:1  F5:2 D5:1  B4:2 D5:1  C5:3  r:3";

// Harbour Lights: slow, in F.
const HARBOUR_CHORDS: [&str; 16] = [
    "F", "F", "Dm", "Dm", "Bb", "Bb", "C", "C7", "F", "F", "Am", "Am", "Bb", "C", "F", "F",
];
const HARBOUR_TUNE: &str = "C6:2 A5:2  F5:3 r:1  D6:2 C6:1 A5:1  F5:3 r:1  D6:2 F6:2  D6:2 Bb5:2  C6:1.5 Bb5:.5 A5:1 G5:1  E5:3 r:1
    A5:2 C6:2  F6:3 r:1  E6:2 C6:1 A5:1  E5:3 r:1  F5:1 Bb5:1 D6:2  C6:1 Bb5:1 G5:2  A5:2 C6:2  F5:3 r:1";

impl Piece {
    /// The notes of loop number `pass` (the arrangement changes a little each time round).
    fn score(self, pass: u32) -> Score {
        let mut body = vec![];
        let mut intro = vec![];
        let mut intro_beats = 0.0;
        let e = |v: &mut Vec<Ev>, beat: f32, inst: Inst, note: f32, beats: f32, vol: f32| {
            v.push(Ev {
                beat,
                inst,
                note,
                beats,
                vol,
            })
        };
        let (bpm, swing, beats) = match self {
            Piece::MainLine => {
                for (bar, sym) in MAIN_CHORDS.iter().enumerate() {
                    let b = bar as f32 * 4.0;
                    let (pc, iv) = chord(sym);
                    let root = pc + 36.0 + if pc > 4.0 { 0.0 } else { 12.0 };
                    // boom-chicka: bass on 1 and 3 (root, then the fifth), strummed chord on 2 and 4
                    e(&mut body, b, Inst::Bass, root, 0.9, 0.55);
                    e(
                        &mut body,
                        b + 2.0,
                        Inst::Bass,
                        root + iv[2] - 12.0,
                        0.9,
                        0.5,
                    );
                    for back in [1.0, 3.0] {
                        for (k, n) in voicing(sym, 55.0).iter().enumerate() {
                            e(
                                &mut body,
                                b + back + k as f32 * 0.03,
                                Inst::Pluck,
                                *n,
                                0.9,
                                0.22,
                            );
                        }
                    }
                    // the wheels: ta-tak on 1, ta-tak on 3
                    for at in [0.0, 0.25, 2.0, 2.25] {
                        e(
                            &mut body,
                            b + at,
                            Inst::Clack,
                            60.0,
                            0.1,
                            if at.fract() == 0.0 { 0.35 } else { 0.22 },
                        );
                    }
                    // the chuff: every eighth, leaning on the off-beats
                    for k in 0..8 {
                        let at = k as f32 * 0.5;
                        e(
                            &mut body,
                            b + at,
                            Inst::Brush,
                            60.0,
                            0.2,
                            if k % 2 == 1 { 0.24 } else { 0.12 },
                        );
                    }
                }
                // harmonica tune; the second time round a banjo takes the verses
                let lead = |a: bool| {
                    if pass % 2 == 1 && a {
                        (Inst::Banjo, 0.75, 12.0)
                    } else {
                        (Inst::Reed, 0.32, 0.0)
                    }
                };
                let mut t = 0.0;
                for (part, a) in [
                    (MAIN_A, true),
                    (MAIN_A2, true),
                    (MAIN_B, false),
                    (MAIN_A2, true),
                ] {
                    let (inst, vol, tr) = lead(a);
                    t += melody(&mut body, t, inst, vol, tr, part);
                }
                assert_eq!(t, MAIN_CHORDS.len() as f32 * 4.0);
                (104.0, 0.55, MAIN_CHORDS.len() as f32 * 4.0)
            }
            Piece::NightFreight => {
                // banjo rolls through the chord, a soft bass, the far-off clack of a train
                const ROLL: [usize; 8] = [0, 2, 3, 1, 2, 3, 1, 2];
                for (bar, sym) in NIGHT_CHORDS.iter().enumerate() {
                    let b = bar as f32 * 4.0;
                    let v = voicing(sym, 52.0);
                    let tones = [v[0] - 12.0, v[0], v[1], v[2]];
                    for (k, tone) in ROLL.iter().enumerate() {
                        e(
                            &mut body,
                            b + k as f32 * 0.5,
                            Inst::Pluck,
                            tones[*tone],
                            1.2,
                            if k == 0 { 0.32 } else { 0.22 },
                        );
                    }
                    e(&mut body, b, Inst::Bass, v[0] - 24.0, 1.8, 0.45);
                    e(&mut body, b + 2.0, Inst::Bass, v[0] - 24.0 + 7.0, 1.8, 0.35);
                    for at in [0.0, 0.25] {
                        e(&mut body, b + at, Inst::Clack, 60.0, 0.1, 0.12);
                    }
                    e(&mut body, b + 1.0, Inst::Brush, 60.0, 0.2, 0.06);
                    e(&mut body, b + 3.0, Inst::Brush, 60.0, 0.2, 0.06);
                }
                let t = melody(
                    &mut body,
                    0.0,
                    Inst::Reed,
                    0.3,
                    if pass % 2 == 1 { -12.0 } else { 0.0 },
                    NIGHT_TUNE,
                );
                assert_eq!(t, NIGHT_CHORDS.len() as f32 * 4.0);
                // a whistle somewhere down the valley
                for n in [66.0, 69.0, 73.0] {
                    e(&mut body, 30.0, Inst::Whistle, n - 3.0, 2.5, 0.1);
                }
                (84.0, 0.0, NIGHT_CHORDS.len() as f32 * 4.0)
            }
            Piece::Departure => {
                // the engine pulls out: chuffs that come faster and faster, then the whistle
                let mut t = 0.0;
                let mut gap = 1.2;
                while gap > 0.3 {
                    e(&mut intro, t, Inst::Brush, 60.0, 0.3, 0.35);
                    t += gap;
                    gap *= 0.86;
                }
                for n in [66.0, 69.0, 73.0] {
                    e(&mut intro, t, Inst::Whistle, n, 3.0, 0.32);
                }
                intro_beats = (t + 4.0).ceil();
                for (bar, sym) in DEPART_CHORDS.iter().enumerate() {
                    let b = bar as f32 * 4.0;
                    for n in voicing(sym, 57.0) {
                        e(&mut body, b, Inst::Pad, n, 3.9, 0.14);
                    }
                    let v = voicing(sym, 50.0);
                    for (k, n) in [v[0], v[1], v[2], v[1] + 12.0, v[2], v[1]]
                        .iter()
                        .enumerate()
                    {
                        e(&mut body, b + k as f32 * 0.66, Inst::Pluck, *n, 1.4, 0.2);
                    }
                    e(&mut body, b, Inst::Bass, v[0] - 12.0, 3.5, 0.4);
                    // slow, steady chuffs: a train in no hurry
                    e(&mut body, b + 1.0, Inst::Brush, 60.0, 0.2, 0.08);
                    e(&mut body, b + 3.0, Inst::Brush, 60.0, 0.2, 0.08);
                }
                let t = melody(&mut body, 0.0, Inst::Reed, 0.28, 0.0, DEPART_TUNE);
                assert_eq!(t, DEPART_CHORDS.len() as f32 * 4.0);
                (76.0, 0.0, DEPART_CHORDS.len() as f32 * 4.0)
            }
            Piece::CoalTrainBlues => {
                for (bar, sym) in BLUES_CHORDS.iter().enumerate() {
                    let b = bar as f32 * 4.0;
                    let (pc, _) = chord(sym);
                    let root = pc + 36.0 + if pc > 4.0 { 0.0 } else { 12.0 };
                    // the boogie bass walks up the chord and back: 1 3 5 6 ♭7 6 5 3
                    for (k, iv) in [0.0, 4.0, 7.0, 9.0, 10.0, 9.0, 7.0, 4.0].iter().enumerate() {
                        e(
                            &mut body,
                            b + k as f32 * 0.5,
                            Inst::Bass,
                            root + iv,
                            0.45,
                            if k % 2 == 0 { 0.5 } else { 0.38 },
                        );
                    }
                    // short guitar stabs on 2 and 4
                    for back in [1.0, 3.0] {
                        for n in voicing(sym, 57.0) {
                            e(&mut body, b + back, Inst::Pluck, n, 0.35, 0.18);
                        }
                    }
                    e(&mut body, b + 1.0, Inst::Brush, 60.0, 0.2, 0.22);
                    e(&mut body, b + 3.0, Inst::Brush, 60.0, 0.2, 0.22);
                    for at in [0.0, 0.5, 2.0, 2.5] {
                        e(
                            &mut body,
                            b + at,
                            Inst::Clack,
                            60.0,
                            0.1,
                            if at.fract() == 0.0 { 0.2 } else { 0.12 },
                        );
                    }
                }
                // harmonica first; the banjo answers in the second chorus (swapped next time round)
                let (first, second) = if pass.is_multiple_of(2) {
                    ((Inst::Reed, 0.32, 0.0), (Inst::Banjo, 0.7, 0.0))
                } else {
                    ((Inst::Banjo, 0.7, 0.0), (Inst::Reed, 0.32, -12.0))
                };
                let mut t = 0.0;
                for ((inst, vol, tr), last) in [(first, BLUES_TURN), (second, BLUES_END)] {
                    t += melody(&mut body, t, inst, vol, tr, BLUES_HEAD);
                    t += melody(&mut body, t, inst, vol, tr, last);
                }
                assert_eq!(t, BLUES_CHORDS.len() as f32 * 4.0);
                (92.0, 1.0, t)
            }
            Piece::Express => {
                for (bar, sym) in REEL_CHORDS.iter().enumerate() {
                    let b = bar as f32 * 4.0;
                    let (pc, iv) = chord(sym);
                    let root = pc + 36.0 + if pc > 4.0 { 0.0 } else { 12.0 };
                    e(&mut body, b, Inst::Bass, root, 0.8, 0.55);
                    e(
                        &mut body,
                        b + 2.0,
                        Inst::Bass,
                        root + iv[2] - 12.0,
                        0.8,
                        0.5,
                    );
                    for back in [1.0, 3.0] {
                        for (k, n) in voicing(sym, 55.0).iter().enumerate() {
                            e(
                                &mut body,
                                b + back + k as f32 * 0.02,
                                Inst::Pluck,
                                *n,
                                0.6,
                                0.2,
                            );
                        }
                    }
                    // the wheels on every beat, and steam on every eighth: flat out
                    for k in 0..4 {
                        e(
                            &mut body,
                            b + k as f32,
                            Inst::Clack,
                            60.0,
                            0.1,
                            if k % 2 == 0 { 0.3 } else { 0.2 },
                        );
                    }
                    for k in 0..8 {
                        e(
                            &mut body,
                            b + k as f32 * 0.5,
                            Inst::Brush,
                            60.0,
                            0.15,
                            if k % 2 == 1 { 0.2 } else { 0.1 },
                        );
                    }
                }
                // the banjo plays the tune, then the harmonica has a go (an octave down)
                let (inst, vol, tr) = if pass.is_multiple_of(2) {
                    (Inst::Banjo, 0.7, 0.0)
                } else {
                    (Inst::Reed, 0.3, -12.0)
                };
                let t = melody(&mut body, 0.0, inst, vol, tr, REEL_TUNE);
                assert_eq!(t, REEL_CHORDS.len() as f32 * 4.0);
                (132.0, 0.0, t)
            }
            Piece::ValleyWaltz => {
                for (bar, sym) in WALTZ_CHORDS.iter().enumerate() {
                    let b = bar as f32 * 3.0;
                    let v = voicing(sym, 55.0);
                    // oom-pah-pah: the root (the fifth every other bar), then the chord twice
                    let bass = if bar % 2 == 0 {
                        v[0] - 24.0
                    } else {
                        v[0] - 24.0 + 7.0
                    };
                    e(&mut body, b, Inst::Bass, bass, 1.0, 0.5);
                    for beat in [1.0, 2.0] {
                        for n in &v {
                            e(&mut body, b + beat, Inst::Pluck, *n, 0.8, 0.15);
                        }
                    }
                    for n in &v {
                        e(&mut body, b, Inst::Pad, *n, 2.9, 0.06);
                    }
                    e(&mut body, b, Inst::Brush, 60.0, 0.2, 0.1);
                }
                // harmonica, then a guitar the second time round
                let (inst, vol, tr) = if pass.is_multiple_of(2) {
                    (Inst::Reed, 0.3, 0.0)
                } else {
                    (Inst::Pluck, 0.55, 0.0)
                };
                let t = melody(&mut body, 0.0, inst, vol, tr, WALTZ_TUNE);
                assert_eq!(t, WALTZ_CHORDS.len() as f32 * 3.0);
                (126.0, 0.0, t)
            }
            Piece::HarbourLights => {
                for (bar, sym) in HARBOUR_CHORDS.iter().enumerate() {
                    let b = bar as f32 * 4.0;
                    for n in voicing(sym, 53.0) {
                        e(&mut body, b, Inst::Pad, n, 3.9, 0.12);
                    }
                    // a guitar picks the chord slowly upward
                    let v = voicing(sym, 48.0);
                    for (k, n) in [v[0], v[1], v[2], v[0] + 12.0, v[2], v[1], v[2], v[0] + 12.0]
                        .iter()
                        .enumerate()
                    {
                        e(
                            &mut body,
                            b + k as f32 * 0.5,
                            Inst::Pluck,
                            *n,
                            1.2,
                            if k == 0 { 0.24 } else { 0.16 },
                        );
                    }
                    e(&mut body, b, Inst::Bass, v[0] - 12.0, 3.6, 0.35);
                }
                // a bell on the quay every few bars
                for bar in [3.0, 11.0] {
                    e(&mut body, bar * 4.0 + 3.0, Inst::Bell, 84.0, 2.0, 0.08);
                }
                let (inst, vol) = if pass.is_multiple_of(2) {
                    (Inst::Reed, 0.26)
                } else {
                    (Inst::Banjo, 0.5)
                };
                let t = melody(&mut body, 0.0, inst, vol, 0.0, HARBOUR_TUNE);
                assert_eq!(t, HARBOUR_CHORDS.len() as f32 * 4.0);
                (70.0, 0.0, t)
            }
        };
        for ev in body.iter_mut() {
            // swing the off-beat eighths late
            if (ev.beat * 2.0).fract() == 0.0 && ev.beat.fract() == 0.5 {
                ev.beat += swing / 6.0;
            }
        }
        body.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        intro.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        Score {
            bpm,
            intro,
            intro_beats: if pass == 0 { intro_beats } else { 0.0 },
            body,
            beats,
        }
    }

    /// How many times round before the next piece, and which piece that is.
    fn next(self) -> (u32, Piece) {
        // lively and calm take turns, so nothing comes round for a quarter of an hour
        match self {
            Piece::Departure => (u32::MAX, Piece::Departure),
            Piece::MainLine => (2, Piece::ValleyWaltz),
            Piece::ValleyWaltz => (2, Piece::CoalTrainBlues),
            Piece::CoalTrainBlues => (2, Piece::NightFreight),
            Piece::NightFreight => (2, Piece::Express),
            Piece::Express => (2, Piece::HarbourLights),
            Piece::HarbourLights => (2, Piece::MainLine),
        }
    }
}

/// Plays pieces: turns the score into voices at the right moments.
struct Music {
    piece: Piece,
    pass: u32,
    score: Score,
    /// Samples since the start of this pass (intro included).
    t: u64,
    next: usize,
    in_intro: bool,
    voices: Vec<Voice>,
    /// Current and target loudness, for smooth pauses and changes of piece.
    gain: f32,
    target: f32,
    drums: bool,
    /// Waiting to switch to this piece once faded out.
    pending: Option<Piece>,
}

impl Music {
    fn new(piece: Piece) -> Music {
        Music {
            piece,
            pass: 0,
            score: piece.score(0),
            t: 0,
            next: 0,
            in_intro: true,
            voices: vec![],
            gain: 1.0,
            target: 1.0,
            drums: true,
            pending: None,
        }
    }

    fn start(&mut self, piece: Piece, pass: u32) {
        self.piece = piece;
        self.pass = pass;
        self.score = piece.score(pass);
        self.t = 0;
        self.next = 0;
        self.in_intro = !self.score.intro.is_empty() && pass == 0;
    }

    fn mood(&mut self, m: Mood) {
        let want = match m {
            Mood::Title => Piece::Departure,
            Mood::Running | Mood::Paused => {
                if self.piece == Piece::Departure {
                    Piece::MainLine
                } else {
                    self.piece
                }
            }
        };
        self.drums = m != Mood::Paused;
        self.target = if m == Mood::Paused {
            B::MUSIC_PAUSED_GAIN
        } else {
            1.0
        };
        if want != self.piece {
            self.pending = Some(want);
        }
    }

    fn next(&mut self, n: &mut u32) -> f32 {
        // fade towards the target; a pending piece fades out, then starts fresh
        let goal = if self.pending.is_some() {
            0.0
        } else {
            self.target
        };
        self.gain += (goal - self.gain) * (1.0 / (RATE as f32 * 0.4));
        if let Some(p) = self.pending
            && self.gain < 0.01
        {
            self.pending = None;
            self.voices.clear();
            self.start(p, 0);
        }
        let spb = RATE as f32 * 60.0 / self.score.bpm; // samples per beat
        let beat = self.t as f32 / spb;
        let events = if self.in_intro {
            &self.score.intro
        } else {
            &self.score.body
        };
        while let Some(ev) = events.get(self.next) {
            if ev.beat > beat {
                break;
            }
            if self.drums || !ev.inst.is_drum() {
                let dur = ev.beats * 60_000.0 / self.score.bpm;
                self.voices
                    .push(voice(0.0, ev.inst, ev.note, dur.max(10.0), ev.vol));
            }
            self.next += 1;
        }
        let end = if self.in_intro {
            self.score.intro_beats
        } else {
            self.score.beats
        };
        self.t += 1;
        if self.t as f32 / spb >= end {
            if self.in_intro {
                self.in_intro = false;
                self.t = 0;
                self.next = 0;
            } else {
                let (loops, after) = self.piece.next();
                let pass = self.pass + 1;
                if pass >= loops {
                    self.start(after, 0)
                } else {
                    self.start(self.piece, pass)
                }
            }
        }
        let mut out = 0.0;
        self.voices.retain_mut(|v| match v.next(n) {
            Some(s) => {
                out += s;
                true
            }
            None => false,
        });
        out * self.gain
    }
}

/// `railhead --music-wav DIR`: write a minute or two of each piece to WAV
/// files, to listen to without playing (or for a trailer).
pub fn write_previews(dir: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut out = vec![];
    for piece in Piece::ALL {
        // two times round (the second arrangement differs), plus the intro
        let secs = (piece.score(0).intro_beats + 2.0 * piece.score(0).beats) * 60.0
            / piece.score(0).bpm
            + 1.0;
        let name = piece.file_name();
        let mut m = Music::new(piece);
        let mut n = 0x1234_5678u32;
        let samples: Vec<i16> = (0..(secs * RATE as f32) as usize)
            .map(|_| {
                let x = m.next(&mut n) * B::MUSIC_VOLUME * 4.0; // previews play louder than in-game ambience
                (x / (1.0 + x.abs()) * i16::MAX as f32) as i16
            })
            .collect();
        let mut wav = Vec::with_capacity(44 + samples.len() * 2);
        let data_len = (samples.len() * 2) as u32;
        wav.extend(b"RIFF");
        wav.extend((36 + data_len).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes()); // PCM
        wav.extend(1u16.to_le_bytes()); // mono
        wav.extend(RATE.to_le_bytes());
        wav.extend((RATE * 2).to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        for s in samples {
            wav.extend(s.to_le_bytes());
        }
        let path = dir.join(format!("{name}.wav"));
        std::fs::write(&path, wav)?;
        out.push(path);
    }
    Ok(out)
}

pub struct Synth {
    rx: Receiver<Cmd>,
    voices: Vec<Voice>,
    music: Music,
    music_on: bool,
    /// Gains for the music and the effects (`Levels::gains`).
    music_gain: f32,
    sfx_gain: f32,
    noise: u32,
    n: u64,
}

impl Synth {
    fn new(rx: Receiver<Cmd>) -> Synth {
        Synth {
            rx,
            voices: vec![],
            music: Music::new(Piece::Departure),
            music_on: false,
            music_gain: B::MUSIC_VOLUME,
            sfx_gain: B::SFX_VOLUME,
            noise: 0x9E37_79B9,
            n: 0,
        }
    }

    fn sample(&mut self) -> f32 {
        if self.n.is_multiple_of(256) {
            while let Ok(c) = self.rx.try_recv() {
                match c {
                    Cmd::Sfx(s) => {
                        if self.voices.len() < 32 {
                            self.voices.extend(sfx_voices(s));
                        }
                    }
                    Cmd::Levels {
                        music_on,
                        music,
                        sfx,
                    } => (self.music_on, self.music_gain, self.sfx_gain) = (music_on, music, sfx),
                    Cmd::Mood(m) => self.music.mood(m),
                }
            }
        }
        self.n += 1;
        let noise = &mut self.noise;
        let mut fx = 0.0;
        self.voices.retain_mut(|v| match v.next(noise) {
            Some(s) => {
                fx += s;
                true
            }
            None => false,
        });
        let music = if self.music_on {
            self.music.next(&mut self.noise)
        } else {
            0.0
        };
        // a gentle soft-clip instead of a hard one, in case a lot happens at once
        let x = music * self.music_gain + fx * self.sfx_gain;
        x / (1.0 + x.abs())
    }
}

impl Iterator for Synth {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        Some(self.sample())
    }
}

#[cfg(feature = "audio")]
impl rodio::Source for Synth {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        rodio::ChannelCount::new(1).unwrap()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        rodio::SampleRate::new(RATE).unwrap()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Music on at full volume, and off again.
    fn on() -> Cmd {
        Cmd::Levels {
            music_on: true,
            music: B::MUSIC_VOLUME,
            sfx: B::SFX_VOLUME,
        }
    }
    fn off() -> Cmd {
        Cmd::Levels {
            music_on: false,
            music: 0.0,
            sfx: B::SFX_VOLUME,
        }
    }

    #[test]
    fn volume_levels_scale_and_mute() {
        let l = Levels {
            music: 50,
            sfx: 100,
            muted: false,
        };
        let (m, s) = l.gains();
        assert!((m - B::MUSIC_VOLUME / 2.0).abs() < 1e-6 && (s - B::SFX_VOLUME).abs() < 1e-6);
        assert_eq!(Levels { muted: true, ..l }.gains(), (0.0, 0.0));
        assert_eq!(step_percent(95, true), 100);
        assert_eq!(step_percent(5, false), 0);
        assert_eq!(step_percent(50, true), 50 + B::VOLUME_STEP);
        let mut a = Audio::silent();
        a.toggle_mute();
        assert!(
            a.levels.muted && a.levels.music == B::MUSIC_PERCENT_DEFAULT,
            "muting keeps the levels"
        );
    }

    fn rms(s: &mut Synth, secs: f32) -> f32 {
        let n = (RATE as f32 * secs) as usize;
        (s.take(n).map(|x| x * x).sum::<f32>() / n as f32).sqrt()
    }

    #[test]
    fn synth_makes_sound_and_silence() {
        let (tx, rx) = channel();
        let mut s = Synth::new(rx);
        assert_eq!(rms(&mut s, 0.2), 0.0, "quiet until asked");
        tx.send(Cmd::Sfx(Sfx::Horn)).unwrap();
        assert!(rms(&mut s, 0.3) > 0.01, "whistle is audible");
        rms(&mut s, 0.5); // let it finish
        assert_eq!(rms(&mut s, 0.2), 0.0, "effects end");
        tx.send(on()).unwrap();
        assert!(rms(&mut s, 3.0) > 0.001, "music plays");
        tx.send(off()).unwrap();
        s.nth(300);
        assert_eq!(rms(&mut s, 0.2), 0.0, "music stops");
        // every effect renders, stays in range and ends
        for fx in ALL_SFX {
            tx.send(Cmd::Sfx(fx)).unwrap();
            let peak = s
                .by_ref()
                .take(RATE as usize * 2)
                .fold(0f32, |m, x| m.max(x.abs()));
            assert!(peak > 0.0 && peak < 1.0, "{fx:?} peak {peak}");
            assert!(
                s.voices.is_empty(),
                "{fx:?} should be over within two seconds"
            );
        }
    }

    /// Every piece adds up (each bar of tune fills its bar of chords), plays
    /// at a sensible level, and changes when the game asks it to.
    #[test]
    fn music_follows_the_game() {
        for p in Piece::ALL {
            for pass in 0..2 {
                let sc = p.score(pass);
                assert!(
                    sc.body.iter().all(|e| e.beat < sc.beats + 0.5),
                    "{p:?}: notes past the end"
                );
            }
        }
        let (tx, rx) = channel();
        let mut s = Synth::new(rx);
        tx.send(on()).unwrap();
        tx.send(Cmd::Mood(Mood::Title)).unwrap();
        let title = rms(&mut s, 12.0);
        assert!(title > 0.002 && title < 0.2, "title music level {title}");
        tx.send(Cmd::Mood(Mood::Running)).unwrap();
        rms(&mut s, 2.0);
        assert_eq!(s.music.piece, Piece::MainLine, "the game starts Main Line");
        let running = rms(&mut s, 6.0);
        tx.send(Cmd::Mood(Mood::Paused)).unwrap();
        rms(&mut s, 2.0);
        let paused = rms(&mut s, 6.0);
        assert!(
            paused < running,
            "paused ({paused}) is quieter than running ({running})"
        );
        assert_eq!(
            s.music.piece,
            Piece::MainLine,
            "pausing doesn't change the piece"
        );
        // Main Line hands over to the next piece after its loops
        let mut m = Music::new(Piece::MainLine);
        let mut n = 1u32;
        let secs = 2.0 * 128.0 * 60.0 / 104.0 + 1.0;
        for _ in 0..(secs * RATE as f32) as usize {
            m.next(&mut n);
        }
        assert_eq!(m.piece, Piece::MainLine.next().1);
        // and the game goes round every piece but the title's before repeating
        let mut p = Piece::MainLine;
        let mut seen = vec![];
        while !seen.contains(&p) {
            seen.push(p);
            p = p.next().1;
        }
        assert_eq!(p, Piece::MainLine);
        assert_eq!(
            seen.len(),
            Piece::ALL.len() - 1,
            "every game piece in the rotation: {seen:?}"
        );
    }

    /// No piece goes silent, blows up or clips.
    #[test]
    fn pieces_stay_in_range() {
        for p in Piece::ALL {
            let mut m = Music::new(p);
            let mut n = 7u32;
            let xs: Vec<f32> = (0..RATE as usize * 40)
                .map(|_| m.next(&mut n) * B::MUSIC_VOLUME)
                .collect();
            let peak = xs.iter().fold(0f32, |a, x| a.max(x.abs()));
            let rms = (xs.iter().map(|x| x * x).sum::<f32>() / xs.len() as f32).sqrt();
            eprintln!("{p:?}: peak {peak:.3} rms {rms:.4}");
            assert!(xs.iter().all(|x| x.is_finite()), "{p:?} produced NaN");
            assert!(peak < 0.5 && rms > 0.003, "{p:?}: peak {peak} rms {rms}");
            // and it keeps playing: every 5-second stretch has sound in it
            for (i, chunk) in xs.chunks(RATE as usize * 5).enumerate() {
                assert!(
                    chunk.iter().any(|x| x.abs() > 0.002),
                    "{p:?}: silent around {}s",
                    i * 5
                );
            }
        }
    }

    #[test]
    fn note_names() {
        assert_eq!(pitch("C4"), 60.0);
        assert_eq!(pitch("A4"), 69.0);
        assert_eq!(pitch("F#5"), 78.0);
        assert_eq!(pitch("Bb3"), 58.0);
        assert_eq!(voicing("Em", 52.0), vec![52.0, 55.0, 59.0]);
    }
}
