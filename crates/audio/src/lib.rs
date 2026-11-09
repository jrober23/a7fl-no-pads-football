//! Sound for the game -- and the fact that it may not be there.
//!
//! The same binary runs in three places: a window on a desktop with a sound
//! card, a terminal over SSH with none, and the test suite, where thousands of
//! plays are simulated and nothing should be opened at all. So the device is
//! optional, and every call on [`Audio`] is a no-op when it is missing. Nothing
//! upstream has to ask whether the machine can make a noise.
//!
//! Nothing here is loaded from disk. The first sound is generated in memory, so
//! the release keeps shipping as one executable with no asset directory; a
//! recorded crowd can replace it behind the same call when there is one worth
//! shipping.

use rodio::buffer::SamplesBuffer;
use rodio::{ChannelCount, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate};

/// The rate everything generated here is built at. 48 kHz is what every card
/// Windows hands out plays at without resampling.
const SAMPLE_RATE: u32 = 48_000;
/// Mono. A crowd coming up is not a place for a stereo image, and one channel is
/// half the buffer.
const CHANNELS: u16 = 1;
/// How long the crowd swell runs, in seconds.
const SWELL_SECONDS: f32 = 1.7;

/// The game's sound, or the absence of it.
pub struct Audio {
    /// The device sink, held for as long as the game runs: dropping it stops
    /// everything that is playing.
    _sink: Option<MixerDeviceSink>,
    /// One player for the crowd. A sting gets its own place in the queue rather
    /// than cutting off the previous one.
    crowd: Option<Player>,
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
                Audio {
                    _sink: Some(sink),
                    crowd: Some(crowd),
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
        }
    }

    /// True when there is a device to play through.
    pub fn is_audible(&self) -> bool {
        self.crowd.is_some()
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

/// The swell itself: two low-passed noise streams, so it reads as a roar with
/// voices in it rather than as static, under an envelope that rises fast and
/// falls slowly.
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

/// White noise in `-1..=1` from a xorshift, so the swell needs no dependency.
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

    #[test]
    fn the_swell_is_the_same_every_time() {
        // Deterministic noise: a sound the player hears twice should be the
        // sound, not a different recording of one.
        let first: Vec<f32> = crowd_swell().collect();
        let second: Vec<f32> = crowd_swell().collect();
        assert_eq!(first, second);
    }
}
