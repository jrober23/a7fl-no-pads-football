//! What the player chose on the settings screen, remembered between sessions.
//!
//! The file is plain `key=value` text in the user's config directory: it has to
//! survive a crash mid-write without taking the game with it, and it has to be
//! readable when somebody reports a bug.

use std::fs;
use std::path::PathBuf;

/// The sizes the settings screen offers, in the order it offers them. The canvas
/// is 640x360, so every one of these is a whole-number scale of it and nothing
/// gets resampled off a pixel grid. The game opens on 1080p: it is the size a
/// 1080p or 1440p desktop has, and 3x is the largest whole-number step of the
/// canvas that fits it.
pub const RESOLUTIONS: [(usize, usize); 4] = [(960, 540), (1280, 720), (1600, 900), (1920, 1080)];

/// The quarter lengths the settings screen offers, in minutes. The league plays
/// fifteen; the short ones are for a game that has to fit in a lunch break.
pub const QUARTER_CHOICES: [u32; 6] = [4, 6, 8, 10, 12, 15];

/// How many steps the volume rows offer: 0 to this, in tenths. Ten steps is more
/// than a pad can pick out and few enough to walk with the stick.
pub const VOLUME_STEPS: usize = 10;

/// A volume row's reading: a percentage, because that is what a player expects a
/// volume to say.
fn volume_label(step: usize) -> String {
    format!("{}%", step.min(VOLUME_STEPS) * 100 / VOLUME_STEPS)
}

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Index into the camera list the game runs on.
    pub camera: usize,
    pub width: usize,
    pub height: usize,
    /// A borderless window the size of the display, rather than a frame around it.
    pub fullscreen: bool,
    /// The lens the player tuned on the lens screen. 1.0 / 0.0 is the shipped shot.
    pub lens_zoom: f32,
    pub lens_tilt: f32,
    /// 0 easy, 1 normal, 2 hard.
    pub difficulty: usize,
    /// Minutes in a quarter, from [QUARTER_CHOICES].
    pub quarter_minutes: u32,
    /// How loud the music is, 0 to [VOLUME_STEPS]: the front end's playlist, the
    /// match's intro song and the stadium's after-play track.
    pub music_volume: usize,
    /// How loud the effects are, 0 to [VOLUME_STEPS]: every cue that joins a
    /// play, and the hit the settings screen previews.
    pub effects_volume: usize,
    /// How loud the crowd is, 0 to [VOLUME_STEPS]: the room under the play and
    /// the crowd's answers to a score and a big play. It ships full, because a
    /// field with no room under it is the one thing a stadium cannot do.
    pub crowd_volume: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            camera: 0,
            width: RESOLUTIONS[3].0,
            height: RESOLUTIONS[3].1,
            fullscreen: false,
            lens_zoom: 1.0,
            lens_tilt: 0.0,
            difficulty: 0,
            quarter_minutes: 15,
            music_volume: 8,
            effects_volume: 8,
            crowd_volume: VOLUME_STEPS,
        }
    }
}

impl Settings {
    pub fn resolution_index(&self) -> usize {
        RESOLUTIONS
            .iter()
            .position(|&(w, h)| w == self.width && h == self.height)
            .unwrap_or(1)
    }

    pub fn set_resolution(&mut self, i: usize) {
        let (w, h) = RESOLUTIONS[i % RESOLUTIONS.len()];
        self.width = w;
        self.height = h;
    }

    pub fn resolution_label(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }

    /// Where the lens sits, for the settings card.
    pub fn lens_label(&self) -> String {
        format!("{:.2} / {:+.2}", self.lens_zoom, self.lens_tilt)
    }

    /// Where the quarter length sits in the list, for the arrow keys.
    pub fn quarter_index(&self) -> usize {
        QUARTER_CHOICES
            .iter()
            .position(|&m| m == self.quarter_minutes)
            .unwrap_or(QUARTER_CHOICES.len() - 1)
    }

    pub fn set_quarter(&mut self, i: usize) {
        self.quarter_minutes = QUARTER_CHOICES[i % QUARTER_CHOICES.len()];
    }

    pub fn quarter_label(&self) -> String {
        format!("{} MIN", self.quarter_minutes)
    }

    /// A volume row's reading: a percentage, because that is what a player
    /// expects a volume to say.
    pub fn music_label(&self) -> String {
        volume_label(self.music_volume)
    }

    pub fn effects_label(&self) -> String {
        volume_label(self.effects_volume)
    }

    pub fn crowd_label(&self) -> String {
        volume_label(self.crowd_volume)
    }

    /// The same three as a multiplier for the audio crate.
    pub fn music_level(&self) -> f32 {
        self.music_volume as f32 / VOLUME_STEPS as f32
    }

    pub fn effects_level(&self) -> f32 {
        self.effects_volume as f32 / VOLUME_STEPS as f32
    }

    pub fn crowd_level(&self) -> f32 {
        self.crowd_volume as f32 / VOLUME_STEPS as f32
    }

    pub fn display_label(&self) -> &'static str {
        if self.fullscreen {
            "FULLSCREEN"
        } else {
            "WINDOW"
        }
    }

    /// Where the settings live. A machine with no home directory gets the file
    /// beside the game rather than nowhere at all.
    pub fn path() -> PathBuf {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("7on7").join("settings.cfg")
    }

    pub fn text(&self) -> String {
        format!(
            "# 7 on 7 No Pads Football settings. Delete this file to go back to the defaults.\ncamera={}\nwidth={}\nheight={}\nfullscreen={}\nzoom={:.2}\ntilt={:.2}\ndifficulty={}\nquarters={}\nmusic={}\neffects={}\ncrowd={}\n",
            self.camera,
            self.width,
            self.height,
            self.fullscreen as u8,
            self.lens_zoom,
            self.lens_tilt,
            self.difficulty,
            self.quarter_minutes,
            self.music_volume,
            self.effects_volume,
            self.crowd_volume,
        )
    }

    /// Read a settings file, keeping the default for anything missing or
    /// unreadable: a hand-edited file must never stop the game starting.
    pub fn parse(text: &str) -> Settings {
        let mut out = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "camera" => {
                    if let Ok(v) = value.parse::<usize>() {
                        out.camera = v;
                    }
                }
                "width" => {
                    if let Ok(v) = value.parse::<usize>() {
                        out.width = v;
                    }
                }
                "height" => {
                    if let Ok(v) = value.parse::<usize>() {
                        out.height = v;
                    }
                }
                "fullscreen" => {
                    out.fullscreen = matches!(value, "1" | "true" | "yes" | "on");
                }
                "zoom" => {
                    if let Ok(v) = value.parse::<f32>() {
                        out.lens_zoom = v;
                    }
                }
                "tilt" => {
                    if let Ok(v) = value.parse::<f32>() {
                        out.lens_tilt = v;
                    }
                }
                "difficulty" => {
                    if let Ok(v) = value.parse::<usize>() {
                        out.difficulty = v;
                    }
                }
                "quarters" => {
                    if let Ok(v) = value.parse::<u32>() {
                        out.quarter_minutes = v;
                    }
                }
                "music" => {
                    if let Ok(v) = value.parse::<usize>() {
                        out.music_volume = v;
                    }
                }
                "effects" => {
                    if let Ok(v) = value.parse::<usize>() {
                        out.effects_volume = v;
                    }
                }
                "crowd" => {
                    if let Ok(v) = value.parse::<usize>() {
                        out.crowd_volume = v;
                    }
                }
                _ => {}
            }
        }
        out.clamp()
    }

    /// Keep the numbers inside what the game can actually draw. A resolution the
    /// list does not offer is put on the nearest one.
    fn clamp(mut self) -> Settings {
        if self.camera > 8 {
            self.camera = 0;
        }
        self.difficulty %= 3;
        self.music_volume = self.music_volume.min(VOLUME_STEPS);
        self.effects_volume = self.effects_volume.min(VOLUME_STEPS);
        self.crowd_volume = self.crowd_volume.min(VOLUME_STEPS);
        // A hand-edited quarter length snaps to the nearest offered one rather
        // than to a clock the menus cannot walk back to.
        if !QUARTER_CHOICES.contains(&self.quarter_minutes) {
            self.quarter_minutes = *QUARTER_CHOICES
                .iter()
                .min_by_key(|&&m| (m as i32 - self.quarter_minutes as i32).abs())
                .unwrap_or(&15);
        }
        // A hand-edited file must not put the lens inside the turf.
        self.lens_zoom =
            if self.lens_zoom.is_finite() { self.lens_zoom.clamp(0.6, 1.8) } else { 1.0 };
        self.lens_tilt =
            if self.lens_tilt.is_finite() { self.lens_tilt.clamp(-0.35, 0.45) } else { 0.0 };
        if self.width < 320 || self.height < 180 {
            self.width = RESOLUTIONS[1].0;
            self.height = RESOLUTIONS[1].1;
            return self;
        }
        if !RESOLUTIONS.iter().any(|&(w, h)| w == self.width && h == self.height) {
            let want = self.width as f32 / self.height.max(1) as f32;
            let mut best = RESOLUTIONS[1];
            for &(w, h) in RESOLUTIONS.iter() {
                if (w as f32 / h as f32 - want).abs() < (best.0 as f32 / best.1 as f32 - want).abs() {
                    best = (w, h);
                }
            }
            self.width = best.0;
            self.height = best.1;
        }
        self
    }

    pub fn load() -> Settings {
        fs::read_to_string(Self::path())
            .map(|t| Settings::parse(&t))
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).ok();
        }
        // Write beside the real file and swap it in, so a half-written file is
        // never what the next launch reads.
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, self.text()).is_ok() {
            if fs::rename(&tmp, &path).is_err() {
                fs::remove_file(&tmp).ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_trip_keeps_every_setting() {
        let s = Settings {
            camera: 2,
            width: 1600,
            height: 900,
            fullscreen: true,
            lens_zoom: 1.25,
            lens_tilt: 0.1,
            difficulty: 2,
            quarter_minutes: 6,
            music_volume: 3,
            effects_volume: 4,
            crowd_volume: 5,
        };
        let back = Settings::parse(&s.text());
        assert_eq!(back.camera, s.camera);
        assert_eq!(back.width, s.width);
        assert_eq!(back.height, s.height);
        assert_eq!(back.fullscreen, s.fullscreen);
        assert_eq!(back.quarter_minutes, s.quarter_minutes);
        assert_eq!(back.music_volume, s.music_volume);
        assert_eq!(back.effects_volume, s.effects_volume);
        assert_eq!(back.crowd_volume, s.crowd_volume);
        assert_eq!(back.music_label(), "30%");
        assert_eq!(back.crowd_label(), "50%");
    }

    #[test]
    fn a_volume_off_the_scale_comes_back_to_it() {
        let loud = Settings::parse("music=99\neffects=11\ncrowd=42");
        assert_eq!(loud.music_volume, VOLUME_STEPS);
        assert_eq!(loud.effects_volume, VOLUME_STEPS);
        assert_eq!(loud.crowd_volume, VOLUME_STEPS);
        assert_eq!(loud.music_level(), 1.0);
        assert_eq!(loud.crowd_level(), 1.0);
        // A file written before the rows existed still reads as the defaults.
        let old = Settings::parse("camera=1\nquarters=8");
        assert_eq!(old.music_volume, Settings::default().music_volume);
        assert_eq!(old.effects_volume, Settings::default().effects_volume);
        // The crowd row ships full, and a file that predates it reads as full.
        assert_eq!(old.crowd_volume, VOLUME_STEPS);
    }

    #[test]
    fn a_missing_or_broken_file_is_the_defaults() {
        let d = Settings::default();
        for text in ["", "garbage", "camera=banana", "fullscreen=maybe"] {
            let got = Settings::parse(text);
            assert_eq!(got.camera, d.camera, "{text:?}");
            assert_eq!(got.width, d.width, "{text:?}");
            assert!(!got.fullscreen, "{text:?}");
        }
    }

    #[test]
    fn a_resolution_off_the_list_snaps_to_a_listed_one() {
        // Every size the game offers is 16:9, so an odd-shaped request lands on
        // one of them and never on a canvas that has to be resampled.
        for text in ["width=1500\nheight=860", "width=1024\nheight=768", "width=2000\nheight=1000"] {
            let s = Settings::parse(text);
            assert!(RESOLUTIONS.contains(&(s.width, s.height)), "{text}: {}x{}", s.width, s.height);
        }
        let kept = Settings::parse("width=1600\nheight=900");
        assert_eq!((kept.width, kept.height), (1600, 900));
        let tiny = Settings::parse("width=40\nheight=40");
        assert_eq!((tiny.width, tiny.height), RESOLUTIONS[1]);
    }

    #[test]
    fn the_resolution_row_walks_the_list_and_wraps() {
        let mut s = Settings::default();
        assert_eq!(s.resolution_index(), 3);
        s.set_resolution(4);
        assert_eq!(s.resolution_label(), "960x540");
    }
}
