//! Sound for the game -- and the fact that it may not be there.
//!
//! The same binary runs in three places: a window on a desktop with a sound
//! card, a terminal over SSH with none, and the test suite, where thousands of
//! plays are simulated and nothing should be opened at all. So the device is
//! optional, and every call on [`Audio`] is a no-op when it is missing. Nothing
//! upstream has to ask whether the machine can make a noise.
//!
//! Nothing here is loaded from disk by default. The bed under the game and the
//! swell that answers a score are generated in memory, so the release keeps
//! shipping as one executable with no asset directory. A player who brings a
//! `sounds` folder of WAVs of their own gets those instead -- the generated
//! sounds are the fallback, not the requirement.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rodio::buffer::SamplesBuffer;
use rodio::{ChannelCount, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source};

/// The rate everything generated here is built at. 48 kHz is what every card
/// Windows hands out plays at without resampling.
const SAMPLE_RATE: u32 = 48_000;
/// Mono. A crowd is not a place for a stereo image, and one channel is half the
/// buffer.
const CHANNELS: u16 = 1;
/// How long the swell that answers a score runs, in seconds.
const SWELL_SECONDS: f32 = 1.7;
/// How long the smaller answer to a big play lasts, in seconds: points get the
/// whole crowd, a long gain gets a rise that is over before the next snap.
const RISE_SECONDS: f32 = 1.1;
/// The fade on the end of that cut, so a crowd file stopped short of its own
/// ending does not click.
const RISE_FADE: f32 = 0.35;
/// A big play against a score: the board did not move, so the room does not get
/// up all the way.
const RISE_LEVEL: f32 = 0.45;
/// The bed loops over this many seconds. Long enough that the ear stops hearing
/// a period, short enough that the buffer is a couple of megabytes.
const BED_SECONDS: f32 = 8.0;
/// How much of the bed's head is folded into its tail, in seconds, so the loop
/// joins itself without a click.
const BED_CROSSFADE: f32 = 0.75;
/// The bed between plays: the room waiting on the call.
pub const BED_BETWEEN: f32 = 0.9;
/// The bed during the snap: still there, so the picture is not silent, but out
/// of the way of the play.
pub const BED_LIVE: f32 = 0.22;
/// How fast the bed moves to a new level, per sample: about a seventieth of a
/// second to settle, which is a duck rather than a step. A step in a noise bed
/// is a click.
const BED_SMOOTH: f32 = 1.0 / (0.07 * SAMPLE_RATE as f32);
/// The folder a player's own sounds are read from, if it is there.
const SOUND_DIR: &str = "sounds";
/// Where the music track starts before a settings file says otherwise: the same
/// 80% the volume rows ship with.
const MUSIC_DEFAULT: f32 = 0.8;
/// The folder inside `sounds` the front end's playlist is read from.
const PLAYLIST_DIR: &str = "menu";
/// How long the match's intro song takes to fade once the kickoff arrives.
const INTRO_FADE_SECONDS: f32 = 1.4;
/// One sample of that fade. The level walks a straight line down rather than
/// chasing a target it never quite reaches, so the song truly gets to silence.
const INTRO_STEP: f32 = 1.0 / (INTRO_FADE_SECONDS * SAMPLE_RATE as f32);

/// The game's sound, or the absence of it.
pub struct Audio {
    /// The device sink, held for as long as the game runs: dropping it stops
    /// everything that is playing.
    _sink: Option<MixerDeviceSink>,
    /// One player for the crowd. A sting gets its own place in the queue rather
    /// than cutting off the previous one.
    crowd: Option<Player>,
    /// Where the bed's level is being asked to go. The source reads it sample by
    /// sample, so the game only ever stores a number here.
    bed_level: Option<Arc<AtomicU32>>,
    /// The bed's own player, kept alive for the length of the game. Its source
    /// never ends, so it plays until this is dropped. It is also the music bus:
    /// the bed today, a stadium loop when there is one.
    bed: Option<Player>,
    /// The stadium's music, when the player brought a `music.wav`: looped, and
    /// audible only in the lull after a play.
    music: Option<Player>,
    /// The front end's playlist: every WAV in `sounds/menu`, joined into one
    /// running order and looped. The menu starts it and dropping this stops it,
    /// because a match has an intro song of its own.
    menu: Option<Player>,
    /// The match's intro song: `sounds/intro.wav`, played from the top when the
    /// matchup comes up.
    intro: Option<Player>,
    /// The intro song's samples, kept so a second match can play it again.
    intro_clip: Option<Arc<Vec<f32>>>,
    /// How loud the intro is right now. The kickoff turns this down and the source
    /// walks there, which is a fade rather than a cut.
    intro_level: Option<Arc<AtomicU32>>,
    /// The music row's level, held here so the menu's playlist and the stadium's
    /// after-play window both follow the same number.
    music_level: Option<Arc<AtomicU32>>,
    /// Whether the stadium's music is in its after-play window.
    stadium: Option<Arc<AtomicBool>>,
    /// The crowd's answer, when the player brought a `cheer.wav`, already in the
    /// shape everything here plays: mono at [`SAMPLE_RATE`].
    cheer: Option<Arc<Vec<f32>>>,
    /// The crowd's answer to a big play, when the player brought a `rise.wav`:
    /// their own recording, played as it is rather than cut from the score's.
    rise_clip: Option<Arc<Vec<f32>>>,
}

impl Audio {
    /// Open the default output device, or carry on silently without one.
    ///
    /// A machine with no sound card, a container with no audio device and a
    /// remote terminal all land in the same place here: a game that plays fine
    /// and makes no noise.
    pub fn open() -> Audio {
        // The player's own files, if they brought any. Each is optional on its
        // own: a crowd of their own with the generated swell still answering a
        // score is a perfectly reasonable thing to want.
        let room = load_clip("crowd.wav");
        let cheer = load_clip("cheer.wav").map(Arc::new);
        let rise_clip = load_clip("rise.wav").map(Arc::new);
        let music = load_clip("music.wav");
        // The front end's playlist and the match's own song.
        let playlist = load_playlist();
        let intro_clip = load_clip("intro.wav").map(Arc::new);
        match DeviceSinkBuilder::open_default_sink() {
            Ok(mut sink) => {
                // rodio prints a line to stderr when the sink is dropped. This
                // one is dropped on the way out of the process, which is not
                // worth a message on a player's terminal.
                sink.log_on_drop(false);
                let crowd = Player::connect_new(sink.mixer());
                let bed_level = Arc::new(AtomicU32::new(BED_BETWEEN.to_bits()));
                let bed = Player::connect_new(sink.mixer());
                bed.append(match room {
                    Some(samples) => CrowdBed::from_samples(samples, Arc::clone(&bed_level)),
                    None => CrowdBed::new(Arc::clone(&bed_level)),
                });
                // The stadium's music is queued but held silent: it comes up
                // after a play, not before it.
                let track = Player::connect_new(sink.mixer());
                if let Some(samples) = music {
                    track.append(samples_buffer(samples).repeat_infinite());
                    track.set_volume(0.0);
                }
                // The playlist is queued and paused: the menu starts it, so a
                // `sim` or `film` run never hears the front end's music.
                let menu = Player::connect_new(sink.mixer());
                if let Some(samples) = playlist {
                    menu.append(samples_buffer(samples).repeat_infinite());
                    menu.set_volume(0.0);
                    menu.pause();
                }
                // The intro song is queued per match, by `intro_song`.
                let intro = Player::connect_new(sink.mixer());
                Audio {
                    _sink: Some(sink),
                    crowd: Some(crowd),
                    bed_level: Some(bed_level),
                    bed: Some(bed),
                    music: Some(track),
                    menu: Some(menu),
                    intro: Some(intro),
                    intro_clip,
                    intro_level: Some(Arc::new(AtomicU32::new(MUSIC_DEFAULT.to_bits()))),
                    music_level: Some(Arc::new(AtomicU32::new(MUSIC_DEFAULT.to_bits()))),
                    stadium: Some(Arc::new(AtomicBool::new(false))),
                    cheer,
                    rise_clip,
                }
            }
            Err(_) => Audio::silent(),
        }
    }

    /// The same type with no device behind it: tests, the terminal client, and
    /// the `sim`/`film` commands all use this, so the game's sound calls are
    /// identical whether or not anything can hear them.
    pub fn silent() -> Audio {
        Audio {
            _sink: None,
            crowd: None,
            bed_level: None,
            bed: None,
            music: None,
            menu: None,
            intro: None,
            intro_clip: None,
            intro_level: None,
            music_level: None,
            stadium: None,
            cheer: None,
            rise_clip: None,
        }
    }

    /// True when there is a device to play through.
    pub fn is_audible(&self) -> bool {
        self.crowd.is_some()
    }

    /// The room between plays: up under the call sheet, the cards and the pause.
    pub fn bed_up(&self) {
        self.set_bed(BED_BETWEEN);
    }

    /// The ball is live: keep the room present but out of the way.
    pub fn bed_down(&self) {
        self.set_bed(BED_LIVE);
    }

    /// Ask the bed for a level in `0..=1`. It ramps there on its own; nothing is
    /// cut, so no step lands inside the noise as a click.
    pub fn set_bed(&self, level: f32) {
        if let Some(bed) = &self.bed_level {
            bed.store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    /// The music row, on every bus it moves: the menu's playlist, the match's intro
    /// song and the stadium's after-play track. The crowd bed shares it, as it
    /// always has.
    pub fn set_music(&self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        if let Some(row) = &self.music_level {
            row.store(level.to_bits(), Ordering::Relaxed);
        }
        if let Some(bed) = &self.bed {
            bed.set_volume(level);
        }
        if let Some(menu) = &self.menu {
            menu.set_volume(level);
        }
        self.apply_stadium();
    }

    fn music_level_now(&self) -> f32 {
        self.music_level
            .as_ref()
            .map(|level| f32::from_bits(level.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
    }

    /// Start the front end's playlist. It runs from the splash screen until a game
    /// is picked, and the music row moves it.
    pub fn menu_music(&self) {
        if let Some(menu) = &self.menu {
            menu.set_volume(self.music_level_now());
            menu.play();
        }
    }

    /// The stadium's music, on or off: it plays after a play, not before the snap
    /// and not under the introduction, the way a stadium's DJ is on the other side
    /// of the whistle.
    pub fn stadium(&self, on: bool) {
        if let Some(flag) = &self.stadium {
            flag.store(on, Ordering::Relaxed);
        }
        self.apply_stadium();
    }

    fn apply_stadium(&self) {
        let level = self.music_level_now();
        let open = self
            .stadium
            .as_ref()
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false);
        if let Some(music) = &self.music {
            music.set_volume(if open { level } else { 0.0 });
        }
    }

    /// Hold the stadium's music off for as long as the guard lives: the ball is
    /// live, which is the one time the music is not on.
    pub fn during_play(&self) -> Stadium<'_> {
        self.stadium(false);
        Stadium(self)
    }

    /// The match's own song, from the top. It runs under the introduction and is
    /// faded out when the kickoff arrives.
    pub fn intro_song(&self) {
        let (Some(intro), Some(clip), Some(level)) =
            (&self.intro, &self.intro_clip, &self.intro_level)
        else {
            return;
        };
        level.store(MUSIC_DEFAULT.to_bits(), Ordering::Relaxed);
        intro.clear();
        intro.append(IntroClip::new(clip.as_ref().clone(), Arc::clone(level)));
        intro.play();
    }

    /// Fade the intro song out rather than cutting it: the kickoff is coming, and
    /// a record stopped mid-bar sounds like a mistake.
    pub fn intro_fade(&self) {
        if let Some(level) = &self.intro_level {
            level.store(0, Ordering::Relaxed);
        }
    }

    /// The effects bus: the crowd's answer to a score, and the cues that join
    /// it. This is the settings screen's EFFECTS row.
    pub fn set_effects(&self, level: f32) {
        if let Some(crowd) = &self.crowd {
            crowd.set_volume(level.clamp(0.0, 1.0));
        }
    }

    /// Hold the bed down for as long as the guard lives.
    ///
    /// The ball is live from the snap to the whistle, which is a scope rather
    /// than a call: the caller takes this at the top of the play and the room
    /// comes back up on every way out of it, including the early ones.
    pub fn ducked(&self) -> Duck<'_> {
        self.bed_down();
        Duck(self)
    }

    /// The crowd coming up for points on the board.
    ///
    /// Generated rather than recorded: a swell of noise that rises in a tenth of
    /// a second, breathes, and falls away. It is a placeholder for a real crowd
    /// recording, but it is the right gesture -- the room reacting to a score --
    /// and it costs the download nothing.
    pub fn crowd_sting(&self) {
        let Some(crowd) = &self.crowd else {
            return;
        };
        crowd.append(self.swell());
    }

    /// The crowd noticing a big play rather than points: a long gain, a
    /// turnover, a stop on fourth down.
    ///
    /// A player who brought a `rise.wav` hears their own recording, played once
    /// from the start, the way a score gets their `cheer.wav`: a sound somebody
    /// made for the moment should not be cut down to fit it. Without one, the
    /// score's source -- their `cheer.wav`, or the generated swell -- is cut to
    /// [`RISE_SECONDS`], faded out and turned down. Either way it is a smaller
    /// thing on purpose: a crowd that answers every twenty-yard throw the way it
    /// answers six points has nothing left for six points.
    pub fn crowd_rise(&self) {
        let Some(crowd) = &self.crowd else {
            return;
        };
        crowd.append(self.rise());
    }

    /// The crowd's answer to a score.
    fn swell(&self) -> SamplesBuffer {
        samples_buffer(self.swell_samples())
    }

    /// The answer to a big play: the player's own file when they brought one,
    /// and the score's source shortened and turned down when they did not.
    fn rise(&self) -> SamplesBuffer {
        match &self.rise_clip {
            Some(samples) => samples_buffer(samples.as_ref().clone()),
            None => samples_buffer(shortened(self.swell_samples(), RISE_SECONDS, RISE_LEVEL)),
        }
    }

    /// The crowd's answer before it is shaped for a source: the player's own
    /// file when they brought one, and the generated swell otherwise.
    fn swell_samples(&self) -> Vec<f32> {
        match &self.cheer {
            Some(samples) => samples.as_ref().clone(),
            None => crowd_swell(),
        }
    }

    /// One swell on the effects bus, in place of whatever was already playing on
    /// it. This is the settings screen's EFFECTS row: a run of presses should be
    /// heard as a level moving, not as a queue of swells piling up behind each
    /// other.
    pub fn crowd_cue(&self) {
        let Some(crowd) = &self.crowd else {
            return;
        };
        crowd.clear();
        crowd.append(self.swell());
    }
}

/// See [`Audio::ducked`]: dropping this puts the room back up.
pub struct Duck<'a>(&'a Audio);

impl Drop for Duck<'_> {
    fn drop(&mut self) {
        self.0.bed_up();
    }
}

/// See [`Audio::during_play`]: dropping this brings the stadium's music back.
pub struct Stadium<'a>(&'a Audio);

impl Drop for Stadium<'_> {
    fn drop(&mut self) {
        self.0.stadium(true);
    }
}

/// The match's intro song as a `Source`: it plays once, and it reads its level
/// from an atomic every sample, so the kickoff can turn it down without holding
/// the player.
struct IntroClip {
    samples: Vec<f32>,
    at: usize,
    target: Arc<AtomicU32>,
    level: f32,
}

impl IntroClip {
    fn new(samples: Vec<f32>, target: Arc<AtomicU32>) -> IntroClip {
        let level = f32::from_bits(target.load(Ordering::Relaxed));
        IntroClip {
            samples,
            at: 0,
            target,
            level,
        }
    }
}

impl Iterator for IntroClip {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = *self.samples.get(self.at)?;
        let target = f32::from_bits(self.target.load(Ordering::Relaxed));
        // A walk to the target, not a chase: a fade that only approaches silence
        // never arrives, and this one has to end.
        let step = (target - self.level).clamp(-INTRO_STEP, INTRO_STEP);
        self.level = (self.level + step).clamp(0.0, 1.0);
        self.at += 1;
        Some(sample * self.level)
    }
}

impl Source for IntroClip {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(CHANNELS).expect("a mono channel count is not zero")
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SAMPLE_RATE).expect("a sample rate is not zero")
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(
            self.samples.len() as f32 / SAMPLE_RATE as f32,
        ))
    }
}

/// The bed: one period of the room, generated once. The last `BED_CROSSFADE`
/// seconds are folded into the head with an equal-power crossfade, so the sample
/// after the end is the one that followed the start and the loop has no seam.
fn bed_loop() -> Vec<f32> {
    let total = (SAMPLE_RATE as f32 * BED_SECONDS) as usize;
    let fade = (SAMPLE_RATE as f32 * BED_CROSSFADE) as usize;
    let mut raw = Vec::with_capacity(total + fade);
    // Its own seed: the bed and the swell must not be the same noise.
    let mut noise = 0x0BED_2024u32;
    let (mut roar, mut voices) = (0.0f32, 0.0f32);
    for i in 0..(total + fade) {
        let t = i as f32 / SAMPLE_RATE as f32;
        let white = next_noise(&mut noise);
        // A low, slow body and a brighter band over it: a room, not a hiss.
        roar += (white - roar) * 0.02;
        voices += (white - voices) * 0.09;
        let breathing = 0.8 + 0.2 * (t * std::f32::consts::TAU * 0.11).sin();
        raw.push((roar * 2.2 + voices * 0.35) * breathing);
    }
    for i in 0..fade {
        let theta = (i as f32 / fade as f32) * std::f32::consts::FRAC_PI_2;
        raw[i] = raw[i] * theta.sin() + raw[total + i] * theta.cos();
    }
    raw.truncate(total);
    let peak = raw.iter().fold(0.0f32, |loudest, s| loudest.max(s.abs()));
    if peak > 0.0 {
        for sample in &mut raw {
            *sample *= 0.32 / peak;
        }
    }
    raw
}

/// The bed as a `Source`: it never ends, and it reads its level from an atomic
/// every sample so the game never has to hold the player to change it.
struct CrowdBed {
    period: Vec<f32>,
    at: usize,
    target: Arc<AtomicU32>,
    level: f32,
}

impl CrowdBed {
    /// The generated room.
    fn new(target: Arc<AtomicU32>) -> CrowdBed {
        CrowdBed::from_samples(bed_loop(), target)
    }

    /// The same loop over whatever period it is handed: the generated bed, or a
    /// player's own `crowd.wav` read into samples.
    fn from_samples(period: Vec<f32>, target: Arc<AtomicU32>) -> CrowdBed {
        let level = f32::from_bits(target.load(Ordering::Relaxed));
        CrowdBed {
            period,
            at: 0,
            target,
            level,
        }
    }
}

impl Iterator for CrowdBed {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let target = f32::from_bits(self.target.load(Ordering::Relaxed));
        self.level += (target - self.level) * BED_SMOOTH;
        let sample = self.period[self.at] * self.level;
        self.at = (self.at + 1) % self.period.len();
        Some(sample)
    }
}

impl Source for CrowdBed {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(CHANNELS).expect("a mono channel count is not zero")
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SAMPLE_RATE).expect("a sample rate is not zero")
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// The swell that answers a score: two low-passed noise streams, so it reads as
/// a roar with voices in it rather than as static, under an envelope that rises
/// fast and falls slowly.
fn crowd_swell() -> Vec<f32> {
    let total = (SAMPLE_RATE as f32 * SWELL_SECONDS) as usize;
    let mut samples = Vec::with_capacity(total);
    // A seeded generator, not `rand`: the same swell every time makes it
    // something a test can look at.
    let mut noise = 0x5EED_1234u32;
    let (mut roar, mut voices) = (0.0f32, 0.0f32);
    for i in 0..total {
        let t = i as f32 / SAMPLE_RATE as f32;
        let white = next_noise(&mut noise);
        // One pole each: the dull body of a crowd, and a brighter band for the
        // shouts and claps on top of it.
        roar += (white - roar) * 0.035;
        voices += (white - voices) * 0.18;
        let body = roar * 3.0 + voices * 0.55;
        // A tenth of a second up, most of a second down.
        let attack = (t / 0.09).min(1.0);
        let release = ((SWELL_SECONDS - t) / 0.9).clamp(0.0, 1.0).powf(1.4);
        // A slow wobble, so the crowd breathes instead of hissing.
        let wobble = 1.0 + 0.18 * (t * std::f32::consts::TAU * 3.1).sin();
        samples.push((body * attack * release * wobble * 0.32).clamp(-1.0, 1.0));
    }
    samples
}

/// The head of a crowd sound, turned down, and faded out over its last
/// [`RISE_FADE`] seconds.
///
/// Cutting a swell off at a fixed length is how a big play gets a shorter
/// answer than a score without a second generator, but a cut is a step in a
/// waveform and a step is a click: the tail is what makes it an ending.
fn shortened(mut samples: Vec<f32>, seconds: f32, level: f32) -> Vec<f32> {
    let want = (SAMPLE_RATE as f32 * seconds).max(1.0) as usize;
    samples.truncate(want);
    let total = samples.len();
    let fade = ((SAMPLE_RATE as f32 * RISE_FADE) as usize).clamp(1, total.max(1));
    for (i, sample) in samples.iter_mut().enumerate() {
        let left = total.saturating_sub(i) as f32 / fade as f32;
        *sample *= level * left.min(1.0);
    }
    samples
}

/// White noise in `-1..=1` from a xorshift, so none of this needs a dependency.
fn next_noise(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Where a player's own sound is looked for, in the order it is looked for: a
/// `sounds` folder beside the executable, so the unzipped release works wherever
/// it is put, and then one in the directory the game was started from, which is
/// where `cargo run` finds it.
fn sound_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        roots.push(dir);
    }
    if let Ok(dir) = std::env::current_dir() {
        roots.push(dir);
    }
    roots
}

fn sound_path(name: &str) -> Option<PathBuf> {
    sound_roots()
        .into_iter()
        .map(|root| root.join(SOUND_DIR).join(name))
        .find(|path| path.is_file())
}

/// The front end's playlist: every WAV in `sounds/menu`, joined into one running
/// order the game picks, so the menu has more than one track and does not open on
/// the same one every evening. A single `sounds/menu.wav` is a playlist of one, so
/// a player with one track does not have to make a folder for it.
fn load_playlist() -> Option<Vec<f32>> {
    let clips = playlist_clips();
    if clips.is_empty() {
        return None;
    }
    let order = rotate(clips, playlist_seed());
    let total: usize = order.iter().map(|clip| clip.len()).sum();
    let mut out = Vec::with_capacity(total);
    for clip in order {
        out.extend(clip);
    }
    (!out.is_empty()).then_some(out)
}

/// Every track of the playlist, in the order the files were found: the first
/// `sounds/menu` folder either root has.
fn playlist_clips() -> Vec<Vec<f32>> {
    for root in sound_roots() {
        let dir = root.join(SOUND_DIR).join(PLAYLIST_DIR);
        if !dir.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .map(|ext| ext.eq_ignore_ascii_case("wav"))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        let clips: Vec<Vec<f32>> = files.iter().filter_map(|path| load_clip_at(path)).collect();
        if !clips.is_empty() {
            return clips;
        }
    }
    load_clip("menu.wav").into_iter().collect()
}

/// A running order for the playlist: Fisher-Yates from a seed, so a second evening
/// of play does not open on the same track. The tests seed it, so the order can be
/// checked.
fn rotate(clips: Vec<Vec<f32>>, mut seed: u32) -> Vec<Vec<f32>> {
    let mut order = clips;
    for i in (1..order.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let j = (seed as usize) % (i + 1);
        order.swap(i, j);
    }
    order
}

fn playlist_seed() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.subsec_nanos())
        .unwrap_or(0xA7F1_2026)
        | 1
}

/// A player's sound by name, or nothing when they did not bring one.
fn load_clip(name: &str) -> Option<Vec<f32>> {
    load_clip_at(&sound_path(name)?)
}

fn load_clip_at(path: &Path) -> Option<Vec<f32>> {
    let samples = read_wav(path)?;
    (!samples.is_empty()).then_some(samples)
}

/// A WAV read into the shape everything here plays: mono at [`SAMPLE_RATE`].
///
/// 16, 24 and 32-bit PCM and 32-bit float are the formats an editor exports by
/// default. Anything else is not read, which is not a failure: the game has its
/// own sound to fall back on, and a player who drops in a file it cannot use
/// should get a game, not an error.
fn read_wav(path: &Path) -> Option<Vec<f32>> {
    let mut reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 || spec.bits_per_sample == 0 {
        return None;
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample.min(32);
            let scale = 1.0 / (1i64 << (bits - 1)) as f32;
            if bits <= 16 {
                reader
                    .samples::<i16>()
                    .filter_map(Result::ok)
                    .map(|sample| sample as f32 * scale)
                    .collect()
            } else {
                reader
                    .samples::<i32>()
                    .filter_map(Result::ok)
                    .map(|sample| sample as f32 * scale)
                    .collect()
            }
        }
    };
    // Mono is what the bed and the swell are built in, and what the mixer here
    // is known to take: a stereo file folds down rather than being played at
    // half speed or on one side.
    let channels = spec.channels as usize;
    let frames: Vec<f32> = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect();
    if frames.is_empty() {
        return None;
    }
    if spec.sample_rate == SAMPLE_RATE {
        return Some(frames);
    }
    // Linear interpolation onto the mixer's rate. A crowd does not need better,
    // and the last sample is held for the tail so the end does not step.
    let step = spec.sample_rate as f64 / SAMPLE_RATE as f64;
    let out_len = ((frames.len() as f64) / step).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let at = i as f64 * step;
        let low = at.floor() as usize;
        let frac = (at - at.floor()) as f32;
        let a = frames[low.min(frames.len() - 1)];
        let b = frames[(low + 1).min(frames.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    Some(out)
}

/// A clip as a source the mixer will take.
fn samples_buffer(samples: Vec<f32>) -> SamplesBuffer {
    SamplesBuffer::new(
        ChannelCount::new(CHANNELS).expect("a mono channel count is not zero"),
        SampleRate::new(SAMPLE_RATE).expect("a sample rate is not zero"),
        samples,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silent_audio_makes_no_sound_and_asks_no_questions() {
        let audio = Audio::silent();
        assert!(!audio.is_audible(), "a silent game claims to have a device");
        // The calls the game makes have to be safe without a device: a headless
        // run and a machine with no sound card both come through here.
        audio.crowd_sting();
        audio.crowd_rise();
        audio.crowd_cue();
        audio.bed_down();
        audio.bed_up();
        audio.set_bed(4.0);
        audio.set_music(0.5);
        audio.set_effects(0.5);
        audio.menu_music();
        audio.intro_song();
        audio.intro_fade();
        audio.stadium(true);
        audio.during_play();
    }

    #[test]
    fn the_crowd_swell_rises_and_falls_away() {
        let swell: Vec<f32> = crowd_swell();
        let rate = SAMPLE_RATE as usize;
        assert!(
            swell.len() > rate,
            "the swell is only {} samples long",
            swell.len()
        );
        let peak = |range: std::ops::Range<usize>| {
            swell[range]
                .iter()
                .fold(0.0f32, |loudest, s| loudest.max(s.abs()))
        };
        let head = peak(0..swell.len() / 50);
        let middle = peak(swell.len() / 3..2 * swell.len() / 3);
        let tail = peak(swell.len() - swell.len() / 20..swell.len());
        assert!(
            middle > 0.1,
            "the crowd never gets going: its loudest is {middle}"
        );
        assert!(
            head < middle * 0.4,
            "it opens at full volume: {head} against {middle}"
        );
        assert!(
            tail < middle * 0.4,
            "it never falls away: {tail} against {middle}"
        );
        assert!(
            swell.iter().all(|s| s.is_finite()),
            "the swell has a sample that is not a number"
        );
    }

    #[test]
    fn the_swell_is_the_same_every_time() {
        // Deterministic noise: a sound the player hears twice should be the
        // sound, not a different recording of one.
        let first: Vec<f32> = crowd_swell();
        let second: Vec<f32> = crowd_swell();
        assert_eq!(first, second);
    }

    #[test]
    fn the_duck_comes_back_up_on_every_way_out() {
        let level = Arc::new(AtomicU32::new(BED_BETWEEN.to_bits()));
        let audio = Audio {
            _sink: None,
            crowd: None,
            bed_level: Some(Arc::clone(&level)),
            bed: None,
            music: None,
            menu: None,
            intro: None,
            intro_clip: None,
            intro_level: None,
            music_level: None,
            stadium: None,
            cheer: None,
            rise_clip: None,
        };
        {
            let _ducked = audio.ducked();
            assert_eq!(
                f32::from_bits(level.load(Ordering::Relaxed)),
                BED_LIVE,
                "the snap did not duck the room"
            );
        }
        assert_eq!(
            f32::from_bits(level.load(Ordering::Relaxed)),
            BED_BETWEEN,
            "the room stayed down after the play"
        );
    }

    #[test]
    fn the_bed_loop_joins_itself() {
        let bed = bed_loop();
        assert!(
            bed.len() > SAMPLE_RATE as usize * 4,
            "the bed is only {} samples long",
            bed.len()
        );
        let rms = (bed.iter().map(|s| s * s).sum::<f32>() / bed.len() as f32).sqrt();
        let seam = (bed[0] - bed[bed.len() - 1]).abs();
        // A loop clicks when the step across the join is louder than the steps
        // the signal already makes inside itself. Against the rms it is the
        // wrong comparison: the bright band moves a fair way from one sample to
        // the next, and that is the sound, not a fault in the join.
        let mut steps: Vec<f32> = bed
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .collect();
        steps.sort_by(|a, b| a.partial_cmp(b).expect("a finite step"));
        let largest = *steps.last().expect("the bed is longer than one sample");
        assert!(
            seam <= largest,
            "the loop steps {seam} at the seam, past the {largest} it makes inside"
        );
        assert!(seam < rms, "the seam is a jump: {seam} against an rms of {rms}");
        let peak = bed.iter().fold(0.0f32, |loudest, s| loudest.max(s.abs()));
        assert!(peak <= 0.33, "the bed is too hot at its peak: {peak}");
    }

    #[test]
    fn the_bed_follows_the_level_it_is_given() {
        let target = Arc::new(AtomicU32::new(BED_BETWEEN.to_bits()));
        let mut bed = CrowdBed::new(Arc::clone(&target));
        let mean = |bed: &mut CrowdBed, samples: usize| {
            (0..samples).map(|_| bed.next().unwrap().abs()).sum::<f32>() / samples as f32
        };
        let window = SAMPLE_RATE as usize / 2;
        let between = mean(&mut bed, window);
        target.store(BED_LIVE.to_bits(), Ordering::Relaxed);
        // Let the ramp settle before measuring what it settled on.
        for _ in 0..SAMPLE_RATE as usize / 5 {
            bed.next();
        }
        let live = mean(&mut bed, window);
        assert!(
            live < between * 0.5,
            "the bed did not duck under the snap: {live} against {between}"
        );
        assert!(live > 0.0, "the bed went silent instead of quiet");
        // And back up again, which is what happens on the whistle.
        target.store(BED_BETWEEN.to_bits(), Ordering::Relaxed);
        for _ in 0..SAMPLE_RATE as usize / 5 {
            bed.next();
        }
        let back = mean(&mut bed, window);
        assert!(
            back > live * 1.5,
            "the bed never came back up: {back} against {live}"
        );
    }

    /// A WAV written by the test, so the reader is checked against a real file
    /// rather than against itself.
    fn write_test_wav(path: &Path, rate: u32, channels: u16, samples: &[i16]) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).expect("a temp file");
        for sample in samples {
            writer.write_sample(*sample).expect("a written sample");
        }
        writer.finalize().expect("a finished file");
    }

    fn temp_wav(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("nopads-{}-{}.wav", tag, std::process::id()))
    }

    #[test]
    fn a_stereo_file_folds_to_mono_and_lands_on_the_mixers_rate() {
        let path = temp_wav("stereo");
        // A quarter second of file at a quarter of the rate everything here runs
        // at, so the length it comes back as says whether the rate was honoured.
        let mut data = Vec::new();
        for _ in 0..4 {
            data.push(i16::MAX);
            data.push(0);
        }
        write_test_wav(&path, SAMPLE_RATE / 4, 2, &data);
        let clip = read_wav(&path).expect("the file was not read");
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            clip.len(),
            16,
            "the rate was not honoured: {} samples came back",
            clip.len()
        );
        // A loud left against a silent right folds to half.
        assert!(
            (clip[0] - 0.5).abs() < 0.02,
            "the channels were not folded: {}",
            clip[0]
        );
    }

    #[test]
    fn a_sound_that_is_not_there_is_not_a_failure() {
        let missing = std::env::temp_dir().join("nopads-this-file-is-not-here.wav");
        let _ = std::fs::remove_file(&missing);
        assert!(load_clip_at(&missing).is_none());
        // A file with a header and no samples is the same as no file at all.
        let empty = temp_wav("empty");
        write_test_wav(&empty, SAMPLE_RATE, 1, &[]);
        assert!(load_clip_at(&empty).is_none());
        let _ = std::fs::remove_file(&empty);
    }

    #[test]
    fn a_loaded_room_loops_the_players_file() {
        let target = Arc::new(AtomicU32::new(BED_BETWEEN.to_bits()));
        let mut bed = CrowdBed::from_samples(vec![1.0, 2.0, 3.0], Arc::clone(&target));
        let out: Vec<f32> = (0..4).map(|_| bed.next().expect("a sample")).collect();
        assert!(
            out[1] > out[0] && out[2] > out[1],
            "the file did not play in order: {out:?}"
        );
        assert!(
            out[3] < out[2],
            "the file did not wrap round to its start: {out:?}"
        );
    }

    #[test]
    fn a_big_play_gets_a_shorter_quieter_crowd() {
        let score = crowd_swell();
        let rise = shortened(score.clone(), RISE_SECONDS, RISE_LEVEL);
        assert_eq!(
            rise.len(),
            (SAMPLE_RATE as f32 * RISE_SECONDS) as usize,
            "the rise is not the length it was asked for"
        );
        assert!(
            rise.len() < score.len(),
            "a big play lasts longer than a score"
        );
        let peak = |samples: &[f32]| samples.iter().fold(0.0f32, |loud, s| loud.max(s.abs()));
        let loud = peak(&rise);
        assert!(
            loud > 0.0 && loud < peak(&score) * RISE_LEVEL + 0.01,
            "the rise is as loud as a score: {loud} against {}",
            peak(&score)
        );
        // The cut is an ending: the last sample is silence rather than a step.
        let end = rise.last().copied().unwrap_or(1.0).abs();
        assert!(end < 0.01, "the rise stops on a step: {end}");
    }

    #[test]
    fn a_players_own_rise_is_played_as_it_was_recorded() {
        let own = || Audio {
            _sink: None,
            crowd: None,
            bed_level: None,
            bed: None,
            music: None,
            menu: None,
            intro: None,
            intro_clip: None,
            intro_level: None,
            music_level: None,
            stadium: None,
            cheer: None,
            rise_clip: Some(Arc::new(vec![0.25; 480])),
        };
        let rise: Vec<f32> = own().rise().collect();
        assert_eq!(
            rise.len(),
            480,
            "the file was not played in full: {} samples",
            rise.len()
        );
        assert!(
            rise.iter().all(|s| (s - 0.25).abs() < 1e-6),
            "a file the player brought was shortened or turned down"
        );
        // And with no file of their own, a big play is still the shortened cut
        // of the score's sound rather than nothing or the whole thing.
        let mut audio = own();
        audio.rise_clip = None;
        let fallback: Vec<f32> = audio.rise().collect();
        assert_eq!(
            fallback.len(),
            (SAMPLE_RATE as f32 * RISE_SECONDS) as usize,
            "the fallback is not the cut a big play asks for"
        );
    }

    #[test]
    fn a_playlist_runs_every_track_once() {
        // Four tracks, told apart by their first sample.
        let clips: Vec<Vec<f32>> = (0..4).map(|i| vec![i as f32; 8]).collect();
        for seed in [1u32, 7, 0xDEAD_BEEF, 42] {
            let order = rotate(clips.clone(), seed);
            assert_eq!(order.len(), 4, "seed {seed} lost a track");
            let mut seen: Vec<f32> = order.iter().map(|clip| clip[0]).collect();
            seen.sort_by(|a, b| a.partial_cmp(b).expect("a finite sample"));
            assert_eq!(
                seen,
                vec![0.0, 1.0, 2.0, 3.0],
                "seed {seed} dropped or repeated a track"
            );
        }
    }

    #[test]
    fn the_intro_song_fades_out_when_the_kickoff_comes() {
        let level = Arc::new(AtomicU32::new(MUSIC_DEFAULT.to_bits()));
        let mut song = IntroClip::new(vec![1.0; SAMPLE_RATE as usize * 4], Arc::clone(&level));
        // It opens at the music row's level, not at the file's own.
        let opening = (0..480)
            .map(|_| song.next().expect("a sample"))
            .fold(0.0f32, f32::max);
        assert!(
            (opening - MUSIC_DEFAULT).abs() < 0.01,
            "the song opened at {opening}"
        );
        // The kickoff arrives: the song walks down to silence instead of stopping
        // on a step, which is what a cut sounds like.
        level.store(0, Ordering::Relaxed);
        let fade: Vec<f32> = (0..(SAMPLE_RATE as usize * 13 / 10))
            .map(|_| song.next().expect("a sample"))
            .collect();
        let middle = fade[fade.len() / 2].abs();
        let last = fade[fade.len() - 1].abs();
        assert!(
            middle > 0.0 && middle < MUSIC_DEFAULT * 0.6,
            "the fade is not moving: {middle}"
        );
        assert!(last < 0.01, "the song never faded out: {last}");
    }

    /// Run by hand on a machine that has a sound card:
    /// `cargo test -p nopads_audio -- --ignored --nocapture`. It opens the real
    /// device and says whether it found one; it plays nothing, so it is safe to
    /// run while somebody is listening to something else.
    #[test]
    #[ignore]
    fn the_default_device_opens_on_a_machine_that_has_one() {
        let audio = Audio::open();
        println!("audible: {}", audio.is_audible());
        assert!(
            audio.is_audible(),
            "no output device was opened: this test is for a machine with one"
        );
    }
}
