//! Sound for the game -- and the fact that it may not be there.
//!
//! The same binary runs in three places: a window on a desktop with a sound
//! card, a terminal over SSH with none, and the test suite, where thousands of
//! plays are simulated and nothing should be opened at all. So the device is
//! optional, and every call on [`Audio`] is a no-op when it is missing. Nothing
//! upstream has to ask whether the machine can make a noise.
//!
//! Nothing here is loaded from disk. The bed under the game and the swell that
//! answers a score are generated in memory, so the release keeps shipping as one
//! executable with no asset directory; a recorded crowd can replace either
//! behind the same calls when there is one worth shipping.

use std::sync::atomic::{AtomicU32, Ordering};
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
    /// never ends, so it plays until this is dropped.
    _bed: Option<Player>,
}

impl Audio {
    /// Open the default output device, or carry on silently without one.
    ///
    /// A machine with no sound card, a container with no audio device and a
    /// remote terminal all land in the same place here: a game that plays fine
    /// and makes no noise.
    pub fn open() -> Audio {
        match DeviceSinkBuilder::open_default_sink() {
            Ok(mut sink) => {
                // rodio prints a line to stderr when the sink is dropped. This
                // one is dropped on the way out of the process, which is not
                // worth a message on a player's terminal.
                sink.log_on_drop(false);
                let crowd = Player::connect_new(sink.mixer());
                let bed_level = Arc::new(AtomicU32::new(BED_BETWEEN.to_bits()));
                let bed = Player::connect_new(sink.mixer());
                bed.append(CrowdBed::new(Arc::clone(&bed_level)));
                Audio {
                    _sink: Some(sink),
                    crowd: Some(crowd),
                    bed_level: Some(bed_level),
                    _bed: Some(bed),
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
            _bed: None,
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
        crowd.append(crowd_swell());
    }
}

/// See [`Audio::ducked`]: dropping this puts the room back up.
pub struct Duck<'a>(&'a Audio);

impl Drop for Duck<'_> {
    fn drop(&mut self) {
        self.0.bed_up();
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
    fn new(target: Arc<AtomicU32>) -> CrowdBed {
        let level = f32::from_bits(target.load(Ordering::Relaxed));
        CrowdBed {
            period: bed_loop(),
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
fn crowd_swell() -> SamplesBuffer {
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
    SamplesBuffer::new(
        ChannelCount::new(CHANNELS).expect("a mono channel count is not zero"),
        SampleRate::new(SAMPLE_RATE).expect("a sample rate is not zero"),
        samples,
    )
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
        audio.bed_down();
        audio.bed_up();
        audio.set_bed(4.0);
    }

    #[test]
    fn the_crowd_swell_rises_and_falls_away() {
        let swell: Vec<f32> = crowd_swell().collect();
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
        let first: Vec<f32> = crowd_swell().collect();
        let second: Vec<f32> = crowd_swell().collect();
        assert_eq!(first, second);
    }

    #[test]
    fn the_duck_comes_back_up_on_every_way_out() {
        let level = Arc::new(AtomicU32::new(BED_BETWEEN.to_bits()));
        let audio = Audio {
            _sink: None,
            crowd: None,
            bed_level: Some(Arc::clone(&level)),
            _bed: None,
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
