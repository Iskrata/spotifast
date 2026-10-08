//! How loud the music is and where its beats fall, for the moving album art
//! orbs.
//!
//! It reads the same post-equalizer, pre-volume sound as the visualisers
//! (see [`crate::vis`]), so the volume knob never changes it and a muted
//! song still moves the orbs. Everything is smoothed: the loudness rises
//! over a third of a second and settles over more than a second, and a beat
//! swells and fades rather than flashing.
//!
//! Beats are rises in bass power well above its recent average, at most a
//! few a second, so a kick drum counts and a steady hum or hiss does not.

use librespot_playback::SAMPLE_RATE;

/// Samples read for one step: about 93 ms, so a short kick never falls
/// between two reads at thirty a second, and long enough that the bass of
/// steady noise holds near its average instead of flickering past it.
pub const SAMPLES: usize = 4096;

/// The loudness range mapped to energy 0 to 1, in decibels below full
/// scale: quiet passages sit near the bottom, mastered pop near the top.
const QUIET_DB: f32 = -42.0;
const LOUD_DB: f32 = -10.0;
/// How long the energy takes to rise and to fall, in seconds.
const ENERGY_ATTACK: f32 = 0.35;
const ENERGY_RELEASE: f32 = 1.2;
/// Below this the bass is too faint to call a beat (a mean square, about
/// -40 dB).
const BASS_FLOOR: f32 = 1e-4;
/// The cut-off of the bass filter, in hertz: kick drums and bass lines.
const BASS_HZ: f32 = 160.0;
/// How long the bass average remembers, in seconds.
const BASS_MEMORY: f32 = 1.0;
/// How far above its average, and above the last read, the bass must jump
/// to count as a beat.
const BEAT_RATIO: f32 = 2.2;
const BEAT_RISE: f32 = 1.15;
/// The shortest time between two beats, in seconds: 214 beats a minute.
const BEAT_GAP: f32 = 0.28;
/// How a beat swells and fades, in seconds.
const PULSE_ATTACK: f32 = 0.06;
const PULSE_FADE: f32 = 0.3;
/// How long the beat count remembers, in seconds, which sets how quickly
/// the pace follows a change of tempo.
const PACE_MEMORY: f32 = 4.0;
/// Beats a second that count as full pace: 150 beats a minute.
const FULL_PACE: f32 = 2.5;

/// The music's smoothed loudness, beat pulse and pace.
#[derive(Clone, Copy, Debug, Default)]
pub struct Vibe {
    energy: f32,
    bass_average: f32,
    bass_last: f32,
    since_beat: f32,
    kick: f32,
    pulse: f32,
    beats: f32,
}

impl Vibe {
    /// Moves on by `dt` seconds, listening to `samples` (mono, -1 to 1,
    /// the newest last). No samples is silence. Returns whether a beat
    /// fell in this step.
    pub fn step(&mut self, samples: &[f32], dt: f32) -> bool {
        let dt = dt.max(0.0);
        self.energy = smooth(
            self.energy,
            loudness(samples),
            dt,
            ENERGY_ATTACK,
            ENERGY_RELEASE,
        );
        let bass = bass_power(samples);
        self.since_beat += dt;
        let beat = bass > BASS_FLOOR
            && bass > self.bass_average * BEAT_RATIO
            && bass > self.bass_last * BEAT_RISE
            && self.since_beat >= BEAT_GAP;
        self.bass_last = bass;
        self.bass_average = smooth(self.bass_average, bass, dt, BASS_MEMORY, BASS_MEMORY);
        self.beats *= (-dt / PACE_MEMORY).exp();
        self.kick *= (-dt / PULSE_FADE).exp();
        if beat {
            self.since_beat = 0.0;
            self.beats += 1.0;
            self.kick = 1.0;
        }
        self.pulse = smooth(self.pulse, self.kick, dt, PULSE_ATTACK, PULSE_FADE);
        beat
    }

    /// How loud the music has been lately, from 0 (silence) to 1.
    pub fn energy(&self) -> f32 {
        self.energy
    }

    /// The swell of the latest beat, from 0 to 1.
    pub fn pulse(&self) -> f32 {
        self.pulse
    }

    /// Beats a second over the last few seconds.
    pub fn beats_per_second(&self) -> f32 {
        self.beats / PACE_MEMORY
    }

    /// The tempo as a share of a brisk one, from 0 to 1.
    pub fn pace(&self) -> f32 {
        (self.beats_per_second() / FULL_PACE).min(1.0)
    }

    /// Whether everything has died down, so nothing is left to animate.
    pub fn calm(&self) -> bool {
        self.energy < 0.005 && self.pulse < 0.005
    }
}

/// Moves `current` toward `target` over `dt` seconds, taking `attack`
/// seconds to rise and `release` to fall (each the time to cover about
/// two thirds of the way).
pub fn smooth(current: f32, target: f32, dt: f32, attack: f32, release: f32) -> f32 {
    let time = if target > current { attack } else { release };
    current + (target - current) * (1.0 - (-dt / time).exp())
}

/// How loud `samples` are, from 0 at 42 dB below full scale to 1 at 10 dB
/// below.
pub fn loudness(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let power = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    if power <= 0.0 {
        return 0.0;
    }
    let db = 10.0 * power.log10();
    ((db - QUIET_DB) / (LOUD_DB - QUIET_DB)).clamp(0.0, 1.0)
}

/// The mean square of `samples` below 160 Hz, through a two-pole
/// low-pass filter.
pub fn bass_power(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let k = 1.0 - (-std::f32::consts::TAU * BASS_HZ / SAMPLE_RATE as f32).exp();
    let (mut first, mut second) = (0.0f32, 0.0f32);
    let mut sum = 0.0;
    for sample in samples {
        first += k * (sample - first);
        second += k * (first - second);
        sum += second * second;
    }
    sum / samples.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = SAMPLE_RATE as f32;
    /// Thirty steps a second, as the orbs draw while music plays.
    const DT: f32 = 1.0 / 30.0;

    /// A small deterministic noise source.
    struct Noise(u32);

    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
        }
    }

    /// `seconds` of sound from `sample`, a function of the time in
    /// seconds, read in steps the way the orbs read it. Returns the time
    /// of each beat.
    fn listen(vibe: &mut Vibe, seconds: f32, mut sample: impl FnMut(f32) -> f32) -> Vec<f32> {
        let total = (seconds * RATE) as usize;
        let sound: Vec<f32> = (0..total).map(|i| sample(i as f32 / RATE)).collect();
        let step = (DT * RATE) as usize;
        let mut beats = Vec::new();
        let mut end = step;
        while end <= total {
            let window = &sound[end.saturating_sub(SAMPLES)..end];
            if vibe.step(window, DT) {
                beats.push(end as f32 / RATE);
            }
            end += step;
        }
        beats
    }

    /// A kick drum on every beat at `bpm`: a falling 55 Hz thump.
    fn kicks(bpm: f32) -> impl FnMut(f32) -> f32 {
        let period = 60.0 / bpm;
        move |time| {
            let since = time % period;
            let thump = (std::f32::consts::TAU * 55.0 * since).sin();
            0.8 * thump * (-since / 0.06).exp()
        }
    }

    #[test]
    fn the_energy_rises_with_the_music_and_settles_after_it() {
        let mut vibe = Vibe::default();
        let loud = vec![0.4; SAMPLES];
        vibe.step(&loud, 0.1);
        let early = vibe.energy();
        assert!(early > 0.0 && early < 0.5, "rises gradually: {early}");
        for _ in 0..30 {
            vibe.step(&loud, DT);
        }
        let full = vibe.energy();
        assert!(full > 0.9, "reaches the music's level: {full}");
        vibe.step(&[], 0.1);
        assert!(
            vibe.energy() > full * 0.85,
            "falls more slowly than it rose"
        );
        for _ in 0..150 {
            vibe.step(&[], DT);
        }
        assert!(vibe.energy() < 0.05);
    }

    #[test]
    fn smoothing_takes_longer_to_fall_than_to_rise() {
        let up = smooth(0.0, 1.0, 0.1, 0.1, 1.0);
        let down = 1.0 - smooth(1.0, 0.0, 0.1, 0.1, 1.0);
        assert!(up > 0.6 && down < 0.1, "{up} {down}");
        assert_eq!(smooth(0.5, 0.5, 1.0, 0.1, 1.0), 0.5);
    }

    #[test]
    fn beats_follow_a_kick_drum_at_its_tempo() {
        let mut vibe = Vibe::default();
        let beats = listen(&mut vibe, 12.0, kicks(120.0));
        assert!(
            (22..=25).contains(&beats.len()),
            "about one beat each half second: {beats:?}"
        );
        for pair in beats.windows(2).skip(1) {
            let gap = pair[1] - pair[0];
            assert!((gap - 0.5).abs() < 0.05, "evenly spaced: {beats:?}");
        }
        let rate = vibe.beats_per_second();
        assert!((rate - 2.0).abs() < 0.3, "two beats a second: {rate}");
        assert!(vibe.pace() > 0.6);
    }

    #[test]
    fn a_faster_tempo_gives_a_faster_pace() {
        let mut slow = Vibe::default();
        listen(&mut slow, 12.0, kicks(80.0));
        let mut fast = Vibe::default();
        listen(&mut fast, 12.0, kicks(140.0));
        assert!(
            fast.pace() > slow.pace() + 0.2,
            "{} {}",
            fast.pace(),
            slow.pace()
        );
    }

    #[test]
    fn steady_noise_has_no_beats() {
        for seed in [0x2545_f491, 0x9e37_79b9, 0x1234_5678, 0xdead_beef, 7] {
            let mut vibe = Vibe::default();
            let mut noise = Noise(seed);
            // Its start may count once, as any entrance might.
            listen(&mut vibe, 2.0, |_| 0.3 * noise.next());
            let beats = listen(&mut vibe, 10.0, |_| 0.3 * noise.next());
            assert!(beats.is_empty(), "seed {seed}: {beats:?}");
            assert!(vibe.energy() > 0.5, "noise is still loud");
            assert!(vibe.pulse() < 0.05);
        }
    }

    #[test]
    fn kicks_count_over_a_bass_line_and_hiss() {
        let mut vibe = Vibe::default();
        let mut noise = Noise(0x9e37_79b9);
        let mut kick = kicks(128.0);
        let beats = listen(&mut vibe, 12.0, |time| {
            let bass = 0.15 * (std::f32::consts::TAU * 82.4 * time).sin();
            kick(time) + bass + 0.1 * noise.next()
        });
        let rate = vibe.beats_per_second();
        assert!(beats.len() >= 20, "{beats:?}");
        assert!((rate - 128.0 / 60.0).abs() < 0.4, "{rate}");
    }

    #[test]
    fn a_steady_tone_has_no_beats() {
        let mut vibe = Vibe::default();
        let tone = |time: f32| 0.5 * (std::f32::consts::TAU * 60.0 * time).sin();
        listen(&mut vibe, 1.0, tone);
        assert!(listen(&mut vibe, 10.0, tone).is_empty());
    }

    #[test]
    fn a_beat_swells_and_fades_without_jumping() {
        let mut vibe = Vibe::default();
        let mut previous = 0.0;
        let mut highest = 0.0f32;
        let mut largest_change = 0.0f32;
        let mut kick = kicks(60.0);
        for step in 0..60 {
            let start = step as f32 * DT;
            let window: Vec<f32> = (0..SAMPLES)
                .map(|i| kick(start + i as f32 / RATE))
                .collect();
            vibe.step(&window, DT);
            largest_change = largest_change.max((vibe.pulse() - previous).abs());
            previous = vibe.pulse();
            highest = highest.max(vibe.pulse());
        }
        assert!(highest > 0.4, "a beat shows: {highest}");
        assert!(largest_change < 0.5, "never a flash: {largest_change}");
    }

    #[test]
    fn silence_is_calm() {
        let mut vibe = Vibe::default();
        let beats = listen(&mut vibe, 3.0, |_| 0.0);
        assert!(beats.is_empty());
        assert!(vibe.calm());
        assert_eq!(vibe.pace(), 0.0);
        assert_eq!(loudness(&[]), 0.0);
        assert_eq!(loudness(&[0.0; 64]), 0.0);
    }

    #[test]
    fn the_bass_filter_keeps_the_low_notes() {
        let tone = |hz: f32| -> Vec<f32> {
            (0..SAMPLES)
                .map(|i| (std::f32::consts::TAU * hz * i as f32 / RATE).sin())
                .collect()
        };
        let low = bass_power(&tone(50.0));
        let high = bass_power(&tone(3000.0));
        assert!(low > 0.3, "{low}");
        assert!(high < low * 0.01, "{high}");
    }
}
