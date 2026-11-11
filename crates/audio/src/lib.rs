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
/// The bed between plays: the room waiting on the call, a little under full so
/// the snap is heard as the room coming up rather than as nothing changing.
pub const BED_BETWEEN: f32 = 0.75;
/// The bed during the snap: full. The room is the loudest thing in the stadium
/// while the ball is live, and at a third of this the field sounded empty; the
/// CROWD row on the settings screen is what turns it down.
pub const BED_LIVE: f32 = 1.0;
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
/// The folder the stadium's music is read from: shuffled the same way, so a
/// stadium with more than one track does not play the same record all evening.
/// `sounds/music.wav` is still the one-track fallback.
const STADIUM_DIR: &str = "stadium";
/// The folder the play's own sounds are read from: one file per cue, named for
/// the cue. `sounds/sfx/tackle.wav` is the tackle, and so on.
const SFX_DIR: &str = "sfx";
/// How long the match's intro song takes to fade once the kickoff arrives.
const INTRO_FADE_SECONDS: f32 = 1.4;
/// One sample of that fade. The level walks a straight line down rather than
/// chasing a target it never quite reaches, so the song truly gets to silence.
const INTRO_STEP: f32 = 1.0 / (INTRO_FADE_SECONDS * SAMPLE_RATE as f32);
/// How much of a song one play gets. A stadium's DJ cuts a record for a down
/// and moves on to the next one, so this is a phrase or two rather than the
/// whole track.
const SNIPPET_SECONDS: f32 = 10.0;
/// How the end of that snippet is taken: the last fraction of a second is
/// faded, so a record cut short for the next play ends instead of clicking.
const SNIPPET_TAIL: f32 = 0.6;

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
    /// The stadium's music: every track in `sounds/stadium`, shuffled into one
    /// running order, with `music.wav` as a one-track fallback. Each play gets a
    /// snippet of the next song on that order, from somewhere inside it, rather
    /// than the whole album running end to end under the game.
    music: Option<Player>,
    /// Which snippet is due. The game bumps it when a play ends and the record
    /// comes back; the track's source reads it every sample, so a new number is
    /// a new song from a new spot in it.
    snippet: Option<Arc<AtomicU32>>,
    /// The deal this session's album was shuffled by, so the spot inside each
    /// song is the same for a whole game and different from the last one.
    salt: u32,
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
    /// The stadium's gate: the level the track's source reads every sample. The
    /// window is a number rather than a player volume, so it opens on the
    /// whistle and shuts on the snap wherever the audio thread has got to, and
    /// the track can fade rather than step.
    stadium_level: Option<Arc<AtomicU32>>,
    /// The crowd row's level, held here so the bed under the play and the
    /// crowd's answers to a score follow one number: the settings screen's CROWD
    /// row. It is not the music row, which is what the menu and the stadium's
    /// track follow.
    crowd_level: Option<Arc<AtomicU32>>,
    /// The crowd's answer, when the player brought a `cheer.wav`, already in the
    /// shape everything here plays: mono at [`SAMPLE_RATE`].
    cheer: Option<Arc<Vec<f32>>>,
    /// The crowd's answer to a big play, when the player brought a `rise.wav`:
    /// their own recording, played as it is rather than cut from the score's.
    rise_clip: Option<Arc<Vec<f32>>>,
    /// The play's own sounds, when the player brought them: a throw, a catch, a
    /// tackle, each read from `sounds/sfx` by name. A missing one is generated
    /// when it is asked for, so the fallback costs nothing until a play makes
    /// that sound.
    throw_clip: Option<Arc<Vec<f32>>>,
    catch_clip: Option<Arc<Vec<f32>>>,
    tackle_clip: Option<Arc<Vec<f32>>>,
    /// The player the cues play on. One at a time: every cue clears it, so a
    /// sound is never heard a second after the moment it belongs to.
    sfx: Option<Player>,
    /// Cues played this session. The clock is the seed and this is what makes the
    /// seed move: a run of hits must not be one hit played five times.
    cues_played: AtomicU32,
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
        // The stadium's music, shuffled from its folder the same way the front
        // end's playlist is, so a stadium with a dozen tracks cycles them.
        let music = load_albums(STADIUM_DIR, "music.wav");
        // The front end's playlist and the match's own song.
        let playlist = load_tracks(PLAYLIST_DIR, "menu.wav");
        let intro_clip = load_clip("intro.wav").map(Arc::new);
        // The play's own sounds. Each is optional: the generated version is
        // played when the file is not there.
        let throw_clip = load_effect("throw.wav");
        let catch_clip = load_effect("catch.wav");
        let tackle_clip = load_effect("tackle.wav");
        match DeviceSinkBuilder::open_default_sink() {
            Ok(mut sink) => {
                // rodio prints a line to stderr when the sink is dropped. This
                // one is dropped on the way out of the process, which is not
                // worth a message on a player's terminal.
                sink.log_on_drop(false);
                let crowd = Player::connect_new(sink.mixer());
                let bed_level = Arc::new(AtomicU32::new(BED_BETWEEN.to_bits()));
                // The crowd row ships full: the field is loud until the player
                // says otherwise.
                let crowd_level = Arc::new(AtomicU32::new(1.0f32.to_bits()));
                let bed = Player::connect_new(sink.mixer());
                bed.append(match room {
                    Some(samples) => CrowdBed::from_samples(
                        samples,
                        Arc::clone(&bed_level),
                        Arc::clone(&crowd_level),
                    ),
                    None => CrowdBed::new(Arc::clone(&bed_level), Arc::clone(&crowd_level)),
                });
                // The stadium's music is queued behind its own gate, held at
                // silence: it comes up after a play, not before it.
                let stadium_level = Arc::new(AtomicU32::new(0.0f32.to_bits()));
                let snippet = Arc::new(AtomicU32::new(0));
                let salt = playlist_seed();
                let track = Player::connect_new(sink.mixer());
                if let Some(album) = music {
                    track.append(StadiumClip::new(
                        Arc::new(album),
                        Arc::clone(&stadium_level),
                        Arc::clone(&snippet),
                        salt,
                    ));
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
                // The cues share one player: a throw or a hit starts now, not
                // after whatever was still playing.
                let sfx = Player::connect_new(sink.mixer());
                Audio {
                    _sink: Some(sink),
                    crowd: Some(crowd),
                    bed_level: Some(bed_level),
                    bed: Some(bed),
                    music: Some(track),
                    snippet: Some(snippet),
                    salt,
                    menu: Some(menu),
                    intro: Some(intro),
                    intro_clip,
                    intro_level: Some(Arc::new(AtomicU32::new(MUSIC_DEFAULT.to_bits()))),
                    music_level: Some(Arc::new(AtomicU32::new(MUSIC_DEFAULT.to_bits()))),
                    stadium: Some(Arc::new(AtomicBool::new(false))),
                    stadium_level: Some(stadium_level),
                    crowd_level: Some(crowd_level),
                    cheer,
                    rise_clip,
                    throw_clip,
                    catch_clip,
                    tackle_clip,
                    sfx: Some(sfx),
                    cues_played: AtomicU32::new(0),
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
            snippet: None,
            salt: 0,
            menu: None,
            intro: None,
            intro_clip: None,
            intro_level: None,
            music_level: None,
            stadium: None,
            stadium_level: None,
            crowd_level: None,
            cheer: None,
            throw_clip: None,
            catch_clip: None,
            tackle_clip: None,
            sfx: None,
            cues_played: AtomicU32::new(0),
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

    /// The music row, on every bus that carries music: the menu's playlist, the
    /// match's intro song and the stadium's after-play track. The crowd has a row
    /// of its own -- the field was quiet because the room was riding a volume
    /// meant for music.
    pub fn set_music(&self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        if let Some(row) = &self.music_level {
            row.store(level.to_bits(), Ordering::Relaxed);
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
    ///
    /// Opening it when it was shut is what a new play sounds like: the next song
    /// on the running order, from somewhere inside it, rather than the record
    /// picking up where the last lull left it.
    pub fn stadium(&self, on: bool) {
        let was = self
            .stadium
            .as_ref()
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false);
        if let Some(flag) = &self.stadium {
            flag.store(on, Ordering::Relaxed);
        }
        if on && !was {
            if let Some(next) = &self.snippet {
                next.fetch_add(1, Ordering::Relaxed);
            }
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
        if let Some(gate) = &self.stadium_level {
            gate.store(if open { level } else { 0.0 }.to_bits(), Ordering::Relaxed);
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

    /// The effects bus: the play's own sounds, and the hits the settings screen
    /// previews with. This is the settings screen's EFFECTS row.
    pub fn set_effects(&self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        if let Some(sfx) = &self.sfx {
            sfx.set_volume(level);
        }
    }

    /// The crowd row: the room under the play and the crowd's answers to a
    /// score and a big play. It used to ride the effects row, where turning up a
    /// tackle turned up the stadium with it.
    pub fn set_crowd(&self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        if let Some(row) = &self.crowd_level {
            row.store(level.to_bits(), Ordering::Relaxed);
        }
        if let Some(crowd) = &self.crowd {
            crowd.set_volume(level);
        }
    }

    /// The ball leaving the passer's hand.
    pub fn throw_sound(&self) {
        self.effect(&self.throw_clip, throw_whoosh, false);
    }

    /// A ball coming down in somebody's hands.
    ///
    /// Two catches in a row are two different catches: the pitch and the level
    /// move a little from one to the next.
    pub fn catch_sound(&self) {
        self.effect(&self.catch_clip, catch_slap, true);
    }

    /// The hit landing, moved the same way a catch is, so a run of tackles is a
    /// run of hits rather than one hit on a loop.
    pub fn tackle_sound(&self) {
        self.effect(&self.tackle_clip, tackle_thud, true);
    }

    /// One cue on the effects bus: the player's own file when they brought one,
    /// the generated version when they did not, and silence when there is no
    /// device at all -- which is every headless run.
    fn effect(&self, clip: &Option<Arc<Vec<f32>>>, generated: fn() -> Vec<f32>, vary: bool) {
        let Some(sfx) = &self.sfx else {
            return;
        };
        sfx.clear();
        let samples = match clip {
            Some(samples) => samples.as_ref().clone(),
            None => generated(),
        };
        let samples = if vary {
            let tick = self.cues_played.fetch_add(1, Ordering::Relaxed);
            let (speed, gain) = cue_variation(tick);
            varied(&samples, speed, gain)
        } else {
            samples
        };
        sfx.append(samples_buffer(samples));
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
            *sample *= 0.45 / peak;
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
    crowd: Arc<AtomicU32>,
    level: f32,
}

impl CrowdBed {
    /// The generated room.
    fn new(target: Arc<AtomicU32>, crowd: Arc<AtomicU32>) -> CrowdBed {
        CrowdBed::from_samples(bed_loop(), target, crowd)
    }

    /// The same loop over whatever period it is handed: the generated bed, or a
    /// player's own `crowd.wav` read into samples.
    fn from_samples(period: Vec<f32>, target: Arc<AtomicU32>, crowd: Arc<AtomicU32>) -> CrowdBed {
        let level = f32::from_bits(target.load(Ordering::Relaxed));
        CrowdBed {
            period,
            at: 0,
            target,
            crowd,
            level,
        }
    }
}

impl Iterator for CrowdBed {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        // The room asked for, under the crowd row: full on the field at the row's
        // shipped setting, and down with the row when the player wants it down.
        let target = f32::from_bits(self.target.load(Ordering::Relaxed))
            * f32::from_bits(self.crowd.load(Ordering::Relaxed));
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

/// How fast the stadium's gate opens and shuts, per sample: a record coming up
/// over a third of a second rather than a switch.
const STADIUM_STEP: f32 = 1.0 / (0.35 * SAMPLE_RATE as f32);

/// The stadium's music as a `Source`: a snippet of one song per play, heard
/// through a gate the game opens after a play and shuts on the snap.
///
/// The album is a running order of whole tracks, and a play asks for the next one
/// by bumping `snippet`. The snippet starts somewhere inside its song rather than
/// at the top, because a DJ drops the needle where the good part is: twelve plays
/// of twelve intros is not music. When the snippet ends the source goes quiet
/// rather than honest -- a `Source` that returns `None` is a player that is
/// finished and never plays again -- so there are samples of zero until the next
/// play asks for a record.
///
/// The gate was the player's own volume, which is a number the mixer reads on its
/// own schedule and not a window anyone could see; a level read per sample is the
/// same thing the crowd does, and it lets the window fade in instead of starting
/// on a step.
struct StadiumClip {
    album: Arc<Vec<Vec<f32>>>,
    /// The song playing, where in it, and where this snippet of it ends.
    song: usize,
    at: usize,
    end: usize,
    /// How many samples the tail fade runs over.
    tail: usize,
    gate: Arc<AtomicU32>,
    snippet: Arc<AtomicU32>,
    /// The snippet that is playing, so a bump is noticed on the next sample.
    playing: u32,
    salt: u32,
    /// The draw this play's spots come from: a snippet that runs out while the
    /// lull does moves on to another point on the same draw.
    seed: u32,
    level: f32,
}

impl StadiumClip {
    fn new(
        album: Arc<Vec<Vec<f32>>>,
        gate: Arc<AtomicU32>,
        snippet: Arc<AtomicU32>,
        salt: u32,
    ) -> StadiumClip {
        let level = f32::from_bits(gate.load(Ordering::Relaxed));
        let playing = snippet.load(Ordering::Relaxed);
        let mut clip = StadiumClip {
            album,
            song: 0,
            at: 0,
            end: 0,
            tail: 1,
            gate,
            snippet,
            playing,
            salt,
            seed: 0,
            level,
        };
        clip.choose();
        clip
    }

    /// Take the record as it is asked for: the next song on the running order,
    /// from a spot inside it that is not the beginning.
    fn choose(&mut self) {
        let playing = self.snippet.load(Ordering::Relaxed);
        self.playing = playing;
        self.end = 0;
        let count = self.album.len();
        if count == 0 {
            return;
        }
        // One song a play in the order the album was dealt, so a stadium with
        // three songs plays all three before it repeats one.
        self.song = (playing as usize) % count;
        if self.album[self.song].is_empty() {
            self.song = (0..count)
                .find(|song| !self.album[*song].is_empty())
                .unwrap_or(0);
        }
        // A fresh draw every play, so the same song is a different record.
        self.seed = (self.salt ^ playing.wrapping_mul(0x9E37_79B9)) | 1;
        self.place();
    }

    /// Put the needle on a new spot in the song that is playing: the start of a
    /// snippet, never the start of the record. Called again when a snippet runs
    /// out while the lull is still going, which is how a long wait keeps its
    /// music instead of falling quiet after ten seconds.
    fn place(&mut self) {
        let len = match self.album.get(self.song) {
            Some(song) => song.len(),
            None => return,
        };
        if len == 0 {
            self.end = 0;
            return;
        }
        let want = (SAMPLE_RATE as f32 * SNIPPET_SECONDS) as usize;
        let span = len.saturating_sub(want);
        let start = if span == 0 {
            0
        } else {
            (next_unit(&mut self.seed) * span as f32) as usize
        };
        self.at = start;
        self.end = if span == 0 { len } else { start + want };
        self.tail = ((SAMPLE_RATE as f32 * SNIPPET_TAIL) as usize).clamp(1, self.end - start);
    }
}

impl Iterator for StadiumClip {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.snippet.load(Ordering::Relaxed) != self.playing {
            self.choose();
        }
        let target = f32::from_bits(self.gate.load(Ordering::Relaxed));
        let step = (target - self.level).clamp(-STADIUM_STEP, STADIUM_STEP);
        self.level = (self.level + step).clamp(0.0, 1.0);
        // The snippet is done. With the lull still on, the record moves to
        // another spot in the same song: a player reading the call sheet for a
        // minute hears a minute of music. On the snap it waits, silent, until
        // the next play asks for a record -- and it waits in samples rather
        // than ending, because an iterator that returns nothing is a player
        // that never plays again.
        if self.at >= self.end {
            if target <= 0.0 || self.end == 0 {
                return Some(0.0);
            }
            self.place();
            if self.at >= self.end {
                return Some(0.0);
            }
        }
        let left = self.end - self.at;
        let fade = if left < self.tail {
            left as f32 / self.tail as f32
        } else {
            1.0
        };
        let sample = self
            .album
            .get(self.song)
            .and_then(|song| song.get(self.at))
            .copied()
            .unwrap_or(0.0);
        let out = sample * self.level * fade;
        self.at += 1;
        Some(out)
    }
}

impl Source for StadiumClip {
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
        // A snippet a play: it never finishes on its own, it just goes quiet.
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

/// The fallback for a throw: air moving, not a tone. A short burst of noise with
/// a dull body and a bright edge to it, opening as the arm comes through and gone
/// in a quarter of a second -- the sound of a hand letting go of something.
fn throw_whoosh() -> Vec<f32> {
    const SECONDS: f32 = 0.24;
    let total = (SAMPLE_RATE as f32 * SECONDS) as usize;
    let mut samples = Vec::with_capacity(total);
    let mut noise = 0x7A11_2026u32;
    let (mut low, mut high) = (0.0f32, 0.0f32);
    for i in 0..total {
        let t = i as f32 / SAMPLE_RATE as f32;
        let white = next_noise(&mut noise);
        // Two one-poles: the dull air the arm moves, and the hiss at the edge of
        // the ball. A sweep to the bright band, because the hand is faster at the
        // release than it was a moment before.
        let sweep = (t / 0.055).clamp(0.0, 1.0);
        low += (white - low) * 0.035;
        high += (white - high) * (0.10 + 0.25 * sweep);
        let body = low * 1.7 + high * 0.7;
        // Open in thirty milliseconds, shut by the end: the ball is gone.
        let attack = (t / 0.03).min(1.0);
        let release = ((SECONDS - t) / (SECONDS * 0.75)).clamp(0.0, 1.0).powf(2.2);
        samples.push((body * attack * release * 0.55).clamp(-1.0, 1.0));
    }
    samples
}

/// The fallback for a catch: hands taking a ball that was already moving. The
/// attack is a sample long because a catch is a moment, and there is a dull thump
/// under it so it reads as a body behind the hands rather than a hiss.
fn catch_slap() -> Vec<f32> {
    const SECONDS: f32 = 0.16;
    let total = (SAMPLE_RATE as f32 * SECONDS) as usize;
    let mut samples = Vec::with_capacity(total);
    let mut noise = 0xC47C_2026u32;
    let mut bright = 0.0f32;
    for i in 0..total {
        let t = i as f32 / SAMPLE_RATE as f32;
        let white = next_noise(&mut noise);
        bright += (white - bright) * 0.5;
        let attack = (t / 0.002).min(1.0);
        let release = (-t / 0.045).exp();
        let thump = (t * std::f32::consts::TAU * 150.0).sin() * (-t / 0.05).exp();
        samples.push(((bright * 0.55 * release + thump * 0.35) * attack).clamp(-1.0, 1.0));
    }
    samples
}

/// The fallback for a tackle: a body landing. A low tone that drops as the weight
/// comes down, under the flat noise of two of them hitting the grass.
fn tackle_thud() -> Vec<f32> {
    const SECONDS: f32 = 0.34;
    let total = (SAMPLE_RATE as f32 * SECONDS) as usize;
    let mut samples = Vec::with_capacity(total);
    let mut noise = 0x7AC4_1E00u32;
    let mut body = 0.0f32;
    let mut phase = 0.0f32;
    for i in 0..total {
        let t = i as f32 / SAMPLE_RATE as f32;
        let white = next_noise(&mut noise);
        // A body is a low tone that falls in pitch as it lands, not a drum.
        body += (white - body) * 0.02;
        let hz = 105.0 - 45.0 * (t / 0.12).min(1.0);
        phase += std::f32::consts::TAU * hz / SAMPLE_RATE as f32;
        let thump = phase.sin() * (-t / 0.10).exp();
        let grass = (white - body) * (-t / 0.035).exp();
        let attack = (t / 0.003).min(1.0);
        samples.push(
            ((thump * 0.7 + grass * 0.5 + body * 0.4) * attack).clamp(-1.0, 1.0),
        );
    }
    samples
}

/// How a repeated cue is moved from the last one: a little faster or slower, and
/// never louder than the sound itself, so a run of hits cannot build up.
fn cue_variation(tick: u32) -> (f32, f32) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.subsec_nanos())
        .unwrap_or(0xA7F1_2026);
    let mut seed = (nanos ^ tick.wrapping_mul(0x9E37_79B9)) | 1;
    let speed = 0.92 + 0.16 * next_unit(&mut seed);
    let gain = 0.82 + 0.18 * next_unit(&mut seed);
    (speed, gain)
}

/// A clip as it is heard this time: resampled by `speed` -- above one is shorter
/// and higher -- and scaled by `gain`. Linear interpolation, the same as the
/// reader uses for a file that arrived at another rate.
fn varied(samples: &[f32], speed: f32, gain: f32) -> Vec<f32> {
    let speed = speed.clamp(0.5, 2.0);
    let last = samples.len().saturating_sub(1) as f32;
    let out_len = ((samples.len() as f32) / speed).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let at = (i as f32 * speed).min(last);
        let low = at.floor() as usize;
        let frac = at - at.floor();
        let a = samples[low.min(samples.len() - 1)];
        let b = samples[(low + 1).min(samples.len() - 1)];
        out.push((a + (b - a) * frac) * gain);
    }
    out
}

/// A number in `0..=1` from the same xorshift the noise comes from.
fn next_unit(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x as f32 / u32::MAX as f32
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

/// A folder of tracks joined into one running order the game picks, so a player
/// with more than one track does not hear the same one first every evening: the
/// front end's playlist and the stadium's music are the same thing.
///
/// A single `<single>.wav` is a playlist of one, so a player with one file does
/// not have to make a folder for it.
fn load_tracks(dir: &str, single: &str) -> Option<Vec<f32>> {
    let order = load_albums(dir, single)?;
    let total: usize = order.iter().map(|clip| clip.len()).sum();
    let mut out = Vec::with_capacity(total);
    for clip in order {
        out.extend(clip);
    }
    (!out.is_empty()).then_some(out)
}

/// The same running order kept as whole songs rather than joined into one: the
/// stadium plays a snippet of a different one every play, so it has to know
/// where each song ends and how much of it is worth cutting.
fn load_albums(dir: &str, single: &str) -> Option<Vec<Vec<f32>>> {
    let mut clips = folder_clips(dir);
    if clips.is_empty() {
        clips = load_clip(single).into_iter().collect();
    }
    if clips.is_empty() {
        return None;
    }
    let order = rotate(clips, playlist_seed());
    (!order.is_empty()).then_some(order)
}

/// Every track of a folder, in the order the files were found: the first
/// `sounds/<dir>` folder either root has.
fn folder_clips(dir: &str) -> Vec<Vec<f32>> {
    for root in sound_roots() {
        let dir = root.join(SOUND_DIR).join(dir);
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
    Vec::new()
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

/// One of the play's own sounds, by the cue it is named for, from
/// `sounds/sfx`: nothing when the player did not bring one, and the generated
/// version is played instead.
fn load_effect(name: &str) -> Option<Arc<Vec<f32>>> {
    sound_roots()
        .into_iter()
        .map(|root| root.join(SOUND_DIR).join(SFX_DIR).join(name))
        .find(|path| path.is_file())
        .and_then(|path| load_clip_at(&path))
        .map(Arc::new)
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
        audio.set_crowd(0.5);
        audio.menu_music();
        audio.intro_song();
        audio.intro_fade();
        audio.stadium(true);
        audio.during_play();
        audio.throw_sound();
        audio.catch_sound();
        audio.tackle_sound();
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
            snippet: None,
            salt: 0,
            menu: None,
            intro: None,
            intro_clip: None,
            intro_level: None,
            music_level: None,
            stadium: None,
            stadium_level: None,
            crowd_level: None,
            cheer: None,
            throw_clip: None,
            catch_clip: None,
            tackle_clip: None,
            sfx: None,
            cues_played: AtomicU32::new(0),
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
        assert!(peak <= 0.46, "the bed is too hot at its peak: {peak}");
    }

    #[test]
    fn the_bed_follows_the_level_it_is_given() {
        let target = Arc::new(AtomicU32::new(BED_BETWEEN.to_bits()));
        let crowd = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let mut bed = CrowdBed::new(Arc::clone(&target), Arc::clone(&crowd));
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
            live > between * 1.2,
            "the room did not come up under the snap: {live} against {between}"
        );
        // And down again to the wait for the next call, which is the quieter end
        // of the two now that the field is the loud one.
        target.store(BED_BETWEEN.to_bits(), Ordering::Relaxed);
        for _ in 0..SAMPLE_RATE as usize / 5 {
            bed.next();
        }
        let back = mean(&mut bed, window);
        assert!(
            back < live * 0.85,
            "the room stayed up after the play: {back} against {live}"
        );
        // And the crowd row turns the whole thing down, field and all.
        crowd.store(0.5f32.to_bits(), Ordering::Relaxed);
        target.store(BED_LIVE.to_bits(), Ordering::Relaxed);
        for _ in 0..SAMPLE_RATE as usize / 5 {
            bed.next();
        }
        let halved = mean(&mut bed, window);
        assert!(
            halved < live * 0.65,
            "the crowd row did not move the room: {halved} against {live}"
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
        let crowd = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let mut bed =
            CrowdBed::from_samples(vec![1.0, 2.0, 3.0], Arc::clone(&target), crowd);
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
            snippet: None,
            salt: 0,
            menu: None,
            intro: None,
            intro_clip: None,
            intro_level: None,
            music_level: None,
            stadium: None,
            stadium_level: None,
            crowd_level: None,
            cheer: None,
            throw_clip: None,
            catch_clip: None,
            tackle_clip: None,
            sfx: None,
            cues_played: AtomicU32::new(0),
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

    #[test]
    fn the_stadium_track_waits_for_its_window() {
        let gate = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let snippet = Arc::new(AtomicU32::new(0));
        // Long enough for a whole snippet, so the window is measured on music
        // rather than on the silence after the record has been cut.
        let song = vec![1.0; SAMPLE_RATE as usize * 20];
        let mut track = StadiumClip::new(
            Arc::new(vec![song]),
            Arc::clone(&gate),
            Arc::clone(&snippet),
            7,
        );
        // The ball is live: the record is still turning, and none of it is heard.
        let shut: Vec<f32> = (0..480).map(|_| track.next().expect("a sample")).collect();
        assert!(
            shut.iter().all(|s| *s == 0.0),
            "the stadium music played during the play"
        );
        // The whistle: it comes up over a fade, and it is heard inside the
        // window rather than on the mixer's next pass.
        gate.store(1.0f32.to_bits(), Ordering::Relaxed);
        let opening: Vec<f32> = (0..SAMPLE_RATE as usize / 2)
            .map(|_| track.next().expect("a sample"))
            .collect();
        assert!(
            opening[0].abs() < 0.01,
            "the window opened on a step: {}",
            opening[0]
        );
        let last = opening[opening.len() - 1];
        assert!(
            (last - 1.0).abs() < 0.02,
            "the track never came up: it settled at {last}"
        );
        // Shut again on the snap, and back where it was when the next window
        // opens: the record kept running while nobody was listening.
        gate.store(0, Ordering::Relaxed);
        let quiet: Vec<f32> = (0..SAMPLE_RATE as usize / 2)
            .map(|_| track.next().expect("a sample"))
            .collect();
        let tail = quiet[quiet.len() - 1].abs();
        assert!(tail < 0.01, "the window never shut: {tail}");
    }

    #[test]
    fn a_play_gets_the_next_song_from_somewhere_inside_it() {
        // Three songs, each a ramp up from its own floor, so a sample says both
        // which song it is and how far into it the snippet began.
        let len = SAMPLE_RATE as usize * 30;
        let song = |k: usize| -> Vec<f32> {
            (0..len)
                .map(|i| k as f32 * 0.25 + i as f32 / len as f32 * 0.2)
                .collect()
        };
        let gate = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let snippet = Arc::new(AtomicU32::new(0));
        let mut track = StadiumClip::new(
            Arc::new(vec![song(0), song(1), song(2)]),
            Arc::clone(&gate),
            Arc::clone(&snippet),
            0x5EED_2026,
        );
        let (mut songs, mut spots) = (Vec::new(), Vec::new());
        for _ in 0..6 {
            let opening = track.next().expect("a sample");
            let playing = (opening / 0.25).floor() as usize;
            songs.push(playing);
            spots.push((opening - playing as f32 * 0.25) / 0.2);
            assert!(opening > 0.01, "the snippet was silent");
            // The play ends and the next one is called.
            snippet.fetch_add(1, Ordering::Relaxed);
        }
        // A different song every play, cycling the album in order.
        assert_eq!(songs[..3], [0, 1, 2], "the album was not played in order");
        assert_eq!(songs[3], 0, "the album did not come round again");
        // And never from the top of one: a DJ drops the needle where the song is.
        // A draw can land near a song's beginning, so the question is whether any
        // of them opened at the very top, not how far in they landed.
        assert!(
            spots.iter().all(|spot| *spot > 0.0),
            "a snippet opened at the top: {spots:?}"
        );
        let mut distinct = spots.clone();
        distinct.sort_by(|a, b| a.partial_cmp(b).expect("a finite spot"));
        distinct.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        assert!(
            distinct.len() >= 4,
            "six plays found only {} spots: {spots:?}",
            distinct.len()
        );
        // The lull outlasts the snippet, so the record moves on to another spot
        // in the same song rather than going quiet after ten seconds.
        for _ in 0..((SNIPPET_SECONDS * SAMPLE_RATE as f32) as usize) {
            track.next();
        }
        let resumed: Vec<f32> = (0..480).map(|_| track.next().expect("a sample")).collect();
        assert!(
            resumed.iter().any(|sample| *sample > 0.01),
            "the record went quiet in the middle of a lull"
        );
        // Shut on the snap: quiet until the next play asks for a record.
        gate.store(0, Ordering::Relaxed);
        for _ in 0..(SAMPLE_RATE as usize / 2) {
            track.next();
        }
        let quiet: Vec<f32> = (0..480).map(|_| track.next().expect("a sample")).collect();
        assert!(
            quiet.iter().all(|sample| sample.abs() < 0.01),
            "the record played through the snap"
        );
        // And the next play is another song, because the album is dealt one a play.
        snippet.fetch_add(1, Ordering::Relaxed);
        gate.store(1.0f32.to_bits(), Ordering::Relaxed);
        let next = (0..SAMPLE_RATE as usize / 2)
            .map(|_| track.next().expect("a sample"))
            .fold(0.0f32, f32::max);
        assert!(next > 0.01, "the next play got no record: {next}");
    }

    #[test]
    fn the_record_moves_on_while_the_lull_does() {
        // One long song and the gate open: each snippet is followed by another
        // spot in the same song, so a minute on the call sheet is a minute of
        // music rather than ten seconds and a silence.
        let len = SAMPLE_RATE as usize * 60;
        let song: Vec<f32> = (0..len).map(|i| i as f32 / len as f32).collect();
        let gate = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let snippet = Arc::new(AtomicU32::new(0));
        let mut track = StadiumClip::new(
            Arc::new(vec![song]),
            Arc::clone(&gate),
            Arc::clone(&snippet),
            3,
        );
        let mut spots = Vec::new();
        for _ in 0..4 {
            spots.push(track.next().expect("a sample"));
            for _ in 0..((SNIPPET_SECONDS * SAMPLE_RATE as f32) as usize) {
                let sample = track.next().expect("a sample");
                assert!(sample >= 0.0, "the record fell silent in the lull");
            }
        }
        assert!(
            spots.iter().all(|spot| *spot > 0.0),
            "a snippet opened at the top: {spots:?}"
        );
        let mut distinct = spots.clone();
        distinct.sort_by(|a, b| a.partial_cmp(b).expect("a finite spot"));
        distinct.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        assert!(
            distinct.len() >= 3,
            "the record stayed on one spot: {spots:?}"
        );
    }

    #[test]
    fn a_tackle_that_has_no_file_is_still_a_hit() {
        // The generated stand-ins have to be real sounds: short, audible, and
        // opening and closing on silence so nothing clicks on the way in or out.
        for (name, clip, longest) in [
            ("throw", throw_whoosh(), 0.3),
            ("catch", catch_slap(), 0.2),
            ("tackle", tackle_thud(), 0.4),
        ] {
            let seconds = clip.len() as f32 / SAMPLE_RATE as f32;
            assert!(seconds <= longest, "the {name} lasts {seconds} s");
            assert!(seconds > 0.05, "the {name} is a click, not a sound");
            assert!(
                clip[0].abs() < 0.05,
                "the {name} opens on a step: {}",
                clip[0]
            );
            let peak = clip.iter().fold(0.0f32, |loudest, s| loudest.max(s.abs()));
            assert!(peak > 0.1, "the {name} cannot be heard: peak {peak}");
            assert!(peak <= 1.0, "the {name} clips at {peak}");
            let tail = &clip[clip.len() - 200..];
            assert!(
                tail.iter().all(|s| s.abs() < 0.1),
                "the {name} stops on a step"
            );
            assert!(
                clip.iter().all(|s| s.is_finite()),
                "the {name} has a sample that is not a number"
            );
        }
    }

    #[test]
    fn a_repeated_hit_is_not_the_same_hit() {
        let hits: Vec<(f32, f32)> = (0..8).map(cue_variation).collect();
        for (speed, gain) in &hits {
            assert!((0.9..=1.1).contains(speed), "a hit at {speed} speed");
            assert!((0.8..=1.0).contains(gain), "a hit at {gain} level");
        }
        // Eight hits, eight different pitches: a run of tackles must not be one
        // sample heard eight times.
        let pitches: std::collections::BTreeSet<u32> = hits
            .iter()
            .map(|(speed, _)| (speed * 1000.0) as u32)
            .collect();
        assert!(
            pitches.len() > 5,
            "eight hits came out in only {} pitches",
            pitches.len()
        );
        // And the move is a real resample: a faster hit is shorter, and the
        // level it was given is the level it plays at.
        let same = varied(&[1.0; 480], 1.0, 1.0);
        let faster = varied(&[1.0; 480], 1.06, 0.9);
        assert_eq!(same.len(), 480, "the unchanged hit changed length");
        assert!(
            faster.len() < same.len(),
            "a faster hit was not shorter: {} against {}",
            faster.len(),
            same.len()
        );
        assert!(
            faster.iter().all(|s| (s - 0.9).abs() < 0.01),
            "the level was not applied to the resampled hit"
        );
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
