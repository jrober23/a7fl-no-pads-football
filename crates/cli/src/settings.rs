//! What the player chose on the settings screen, remembered between sessions.
//!
//! The file is plain `key=value` text in the user's config directory: it has to
//! survive a crash mid-write without taking the game with it, and it has to be
//! readable when somebody reports a bug.

use std::fs;
use std::path::PathBuf;

/// The sizes the settings screen offers, in the order it offers them. The canvas
/// is 640x360, so every one of these is a whole-number scale of it and nothing
/// gets resampled off a pixel grid.
pub const RESOLUTIONS: [(usize, usize); 4] = [(960, 540), (1280, 720), (1600, 900), (1920, 1080)];

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Index into the camera list the game runs on.
    pub camera: usize,
    pub width: usize,
    pub height: usize,
    /// A borderless window the size of the display, rather than a frame around it.
    pub fullscreen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { camera: 0, width: RESOLUTIONS[1].0, height: RESOLUTIONS[1].1, fullscreen: false }
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
        base.join("a7fl").join("settings.cfg")
    }

    pub fn text(&self) -> String {
        format!(
            "# A7FL settings. Delete this file to go back to the defaults.\ncamera={}\nwidth={}\nheight={}\nfullscreen={}\n",
            self.camera, self.width, self.height, self.fullscreen as u8
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
        let s = Settings { camera: 2, width: 1600, height: 900, fullscreen: true };
        let back = Settings::parse(&s.text());
        assert_eq!(back.camera, s.camera);
        assert_eq!(back.width, s.width);
        assert_eq!(back.height, s.height);
        assert_eq!(back.fullscreen, s.fullscreen);
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
        assert_eq!(s.resolution_index(), 1);
        s.set_resolution(4);
        assert_eq!(s.resolution_label(), "960x540");
    }
}
