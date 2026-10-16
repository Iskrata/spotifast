//! Live internet radio: one station's stream, played through Spotifast's own
//! output so the equalizer and every visualiser follow it as they follow
//! Spotify. It never plays at the same time as Spotify: starting the radio
//! pauses Spotify, and playing anything from Spotify leaves the radio.
//!
//! The stream is decoded on its own thread and the station's now-playing
//! service is read on another, every [`META_INTERVAL`], while it plays.

use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use librespot_playback::audio_backend::Sink;
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::VolumeGetter;
use librespot_playback::mixer::mappings::MappedCtrl;
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};
use serde::Deserialize;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::backend::Waker;
use crate::resample::Resampler;

/// A station Spotifast can play.
#[derive(Debug, PartialEq, Eq)]
pub struct Station {
    pub name: &'static str,
    /// An Icecast MP3 stream.
    pub stream: &'static str,
    /// The station's now-playing JSON, from the same service its website uses.
    pub meta: &'static str,
    pub logo: &'static str,
}

/// Radio Nova, Sofia. The 128 kbps MP3 is the best stream it offers that
/// this build can decode; its website's player lists the same addresses.
/// Both are plain HTTP: the stream has no other form, and the now-playing
/// server sends an intermediate certificate that does not sign its own
/// (SSL.com's leaf under Sectigo's chain), which only clients that fetch
/// the missing issuer themselves accept.
pub const RADIO_NOVA: Station = Station {
    name: "Radio Nova",
    stream: "http://play.global.audio/nova128",
    meta: "http://meta.metacast.eu/?radio=radionova&songsNumber=1",
    logo: "https://media.metacast.eu/pictures/05_2021/Ws5JjPdSiBDnfyVE70Hq-67892.png",
};

/// The station's website asks every 24 seconds; a little sooner keeps the
/// shown song closer to the one heard.
const META_INTERVAL: Duration = Duration::from_secs(15);
/// How long to wait before reconnecting a stream that failed or ended.
const RETRY_AFTER: Duration = Duration::from_secs(3);
/// How often a sleeping thread looks whether it should stop.
const STOP_POLL: Duration = Duration::from_millis(200);

/// What the radio is doing, for the interface.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RadioState {
    /// The radio is the playback surface, playing or paused.
    pub on: bool,
    pub playing: bool,
    /// Connecting, or waiting for the first sound.
    pub loading: bool,
    pub artist: String,
    pub song: String,
    /// The current song's cover, when the station names one.
    pub image: Option<String>,
}

impl RadioState {
    /// The bar's title and byline: the song and its artist, or, between
    /// songs, the show's name and the station.
    pub fn title_and_byline(&self, station: &Station) -> (String, String) {
        match (self.song.is_empty(), self.artist.is_empty()) {
            (false, false) => (
                self.song.clone(),
                format!("{} \u{b7} {}", self.artist, station.name),
            ),
            (false, true) => (self.song.clone(), station.name.to_string()),
            (true, false) => (self.artist.clone(), station.name.to_string()),
            (true, true) => (station.name.to_string(), String::new()),
        }
    }
}

/// How the radio reaches the output, taken from the Spotify engine's config.
pub struct OutputConfig {
    pub device: Option<String>,
    pub buffer_ms: u32,
    pub tap: Arc<crate::vis::AudioTap>,
    pub eq: crate::eq::SharedEq,
}

/// The radio's volume, on Spotify's 0 to 65535 scale and curve.
#[derive(Clone)]
struct Volume(Arc<AtomicU16>);

impl VolumeGetter for Volume {
    fn attenuation_factor(&self) -> f64 {
        crate::player::VOLUME_CTRL.to_mapped(self.0.load(Ordering::Relaxed))
    }
}

pub struct LiveRadio {
    station: &'static Station,
    state: Arc<Mutex<RadioState>>,
    volume: Volume,
    output: OutputConfig,
    waker: Waker,
    /// Set to end the threads of the stream playing now.
    stop: Option<Arc<AtomicBool>>,
}

impl LiveRadio {
    pub fn new(station: &'static Station, output: OutputConfig, volume: u16, waker: Waker) -> Self {
        Self {
            station,
            state: Arc::default(),
            volume: Volume(Arc::new(AtomicU16::new(volume))),
            output,
            waker,
            stop: None,
        }
    }

    pub fn station(&self) -> &'static Station {
        self.station
    }

    pub fn state(&self) -> RadioState {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn is_on(&self) -> bool {
        self.state().on
    }

    pub fn volume(&self) -> u16 {
        self.volume.0.load(Ordering::Relaxed)
    }

    pub fn set_volume(&self, volume: u16) {
        self.volume.0.store(volume, Ordering::Relaxed);
    }

    /// Starts the stream, reading it through `http`.
    pub fn play(&mut self, http: reqwest::blocking::Client) {
        self.end_stream();
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.on = true;
            state.playing = true;
            state.loading = true;
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Some(Arc::clone(&stop));
        let station = self.station;
        let output = self.output();
        let (state, waker) = (Arc::clone(&self.state), self.waker.clone());
        let stream_http = http.clone();
        let stream_stop = Arc::clone(&stop);
        let spawned = thread::Builder::new()
            .name("live-radio".into())
            .spawn(move || {
                // librespot's sinks stay on the thread that made them.
                let sink = crate::player::own_output(output);
                run_stream(station, &stream_http, &stream_stop, sink, &state, &waker);
            });
        if let Err(error) = spawned {
            log::error!("live radio: could not start the stream thread: {error}");
        }
        let (state, waker) = (Arc::clone(&self.state), self.waker.clone());
        let spawned = thread::Builder::new()
            .name("live-radio-meta".into())
            .spawn(move || run_meta(station, &http, &stop, &state, &waker));
        if let Err(error) = spawned {
            log::error!("live radio: could not start the now-playing thread: {error}");
        }
    }

    /// Pauses: a live stream cannot hold its place, so the stream closes
    /// and Play opens it again at the live edge.
    pub fn pause(&mut self) {
        self.end_stream();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.playing = false;
        state.loading = false;
    }

    /// Leaves the radio for Spotify.
    pub fn leave(&mut self) {
        self.end_stream();
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = RadioState::default();
    }

    fn end_stream(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    fn output(&self) -> crate::player::OwnOutput {
        let report: crate::sink::ErrorHook = Arc::new(|message: String| {
            log::warn!("live radio output: {message}");
        });
        crate::player::OwnOutput {
            device: self.output.device.clone(),
            buffer_ms: self.output.buffer_ms,
            report,
            // The engine saves the device's id when a name opens it; the
            // radio opens whatever the setting holds.
            on_device: None,
            volume: Box::new(self.volume.clone()),
            ceiling: Box::new(self.volume.clone()),
            audio: crate::sink::AudioControl::new(self.output.buffer_ms),
            tap: Arc::clone(&self.output.tap),
            eq: Arc::clone(&self.output.eq),
            // Radio carries no loudness factor; the tap sees it as it is.
            normalisation: Arc::new(AtomicU64::new(1.0f64.to_bits())),
        }
    }
}

impl Drop for LiveRadio {
    fn drop(&mut self) {
        self.end_stream();
    }
}

/// Sleeps for `length`, or less once `stop` is set. Returns whether to stop.
fn wait(stop: &AtomicBool, length: Duration) -> bool {
    let mut left = length;
    while !left.is_zero() {
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        let step = left.min(STOP_POLL);
        thread::sleep(step);
        left -= step;
    }
    stop.load(Ordering::Relaxed)
}

fn update(
    state: &Mutex<RadioState>,
    waker: &Waker,
    stop: &AtomicBool,
    change: impl FnOnce(&mut RadioState),
) {
    // A stream told to stop no longer speaks for the radio: a newer one may.
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
    let before = state.clone();
    change(&mut state);
    if *state != before {
        drop(state);
        waker.wake();
    }
}

/// Plays the station until `stop`, reconnecting when the stream drops.
fn run_stream(
    station: &Station,
    http: &reqwest::blocking::Client,
    stop: &AtomicBool,
    mut sink: Box<dyn Sink>,
    state: &Mutex<RadioState>,
    waker: &Waker,
) {
    let mut started = false;
    while !stop.load(Ordering::Relaxed) {
        let result = play_once(
            station,
            http,
            stop,
            sink.as_mut(),
            &mut started,
            state,
            waker,
        );
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match result {
            Ok(()) => log::info!("live radio: the stream ended; reconnecting"),
            Err(error) => log::warn!("live radio: {error}; reconnecting"),
        }
        update(state, waker, stop, |state| state.loading = true);
        if wait(stop, RETRY_AFTER) {
            break;
        }
    }
    if started && let Err(error) = sink.stop() {
        log::warn!("live radio: could not stop the output: {error}");
    }
}

fn play_once(
    station: &Station,
    http: &reqwest::blocking::Client,
    stop: &AtomicBool,
    sink: &mut dyn Sink,
    started: &mut bool,
    state: &Mutex<RadioState>,
    waker: &Waker,
) -> Result<(), String> {
    let response = http
        .get(station.stream)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("could not open the stream: {}", error.without_url()))?;
    let source =
        MediaSourceStream::new(Box::new(ReadOnlySource::new(response)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| format!("unreadable stream: {error}"))?
        .format;
    let track = format
        .default_track()
        .ok_or_else(|| "the stream has no audio".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| format!("unsupported stream: {error}"))?;
    let mut stereo = Stereo::default();
    let mut converter = Converter::new(None);
    while !stop.load(Ordering::Relaxed) {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(DecodeError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(());
            }
            Err(error) => return Err(format!("stream read failed: {error}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged frame is skipped, as every MP3 player does.
            Err(DecodeError::DecodeError(error)) => {
                log::debug!("live radio: skipped a frame: {error}");
                continue;
            }
            Err(error) => return Err(format!("decoding failed: {error}")),
        };
        let spec = *decoded.spec();
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        let samples = stereo.convert(buffer.samples(), spec.channels.count(), spec.rate);
        if samples.is_empty() {
            continue;
        }
        if !*started {
            sink.start()
                .map_err(|error| format!("could not start the output: {error}"))?;
            *started = true;
        }
        sink.write(AudioPacket::Samples(samples), &mut converter)
            .map_err(|error| format!("the output failed: {error}"))?;
        update(state, waker, stop, |state| state.loading = false);
    }
    Ok(())
}

/// Brings decoded audio to the output's format: stereo at Spotify's rate.
#[derive(Default)]
struct Stereo {
    resampler: Option<(u32, Option<Resampler>)>,
}

impl Stereo {
    fn convert(&mut self, interleaved: &[f32], channels: usize, rate: u32) -> Vec<f64> {
        let channels = channels.max(1);
        let stereo: Vec<f32> = interleaved
            .chunks_exact(channels)
            .flat_map(|frame| {
                let left = frame[0];
                let right = frame.get(1).copied().unwrap_or(left);
                [left, right]
            })
            .collect();
        if self
            .resampler
            .as_ref()
            .is_none_or(|(from, _)| *from != rate)
        {
            self.resampler = Some((
                rate,
                Resampler::new(rate, SAMPLE_RATE, NUM_CHANNELS as usize),
            ));
        }
        let samples = match self
            .resampler
            .as_mut()
            .and_then(|(_, resampler)| resampler.as_mut())
        {
            Some(resampler) => resampler.process(&stereo),
            None => stereo,
        };
        samples.into_iter().map(f64::from).collect()
    }
}

/// The station's now-playing answer.
#[derive(Debug, Default, Deserialize)]
struct Meta {
    #[serde(default)]
    current_artist: Option<String>,
    #[serde(default)]
    current_song: Option<String>,
    #[serde(default)]
    image: Option<String>,
}

fn parse_meta(body: &str) -> Option<Meta> {
    // The service starts its answer with a byte order mark.
    serde_json::from_str(body.trim_start_matches('\u{feff}').trim()).ok()
}

/// Reads what the station is playing until `stop`.
fn run_meta(
    station: &Station,
    http: &reqwest::blocking::Client,
    stop: &AtomicBool,
    state: &Mutex<RadioState>,
    waker: &Waker,
) {
    loop {
        let meta = http
            .get(station.meta)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::text);
        match meta.map(|body| parse_meta(&body)) {
            Ok(Some(meta)) => update(state, waker, stop, |state| {
                state.artist = clean(meta.current_artist);
                state.song = clean(meta.current_song);
                state.image = meta.image.filter(|url| url.starts_with("https://"));
            }),
            Ok(None) => log::debug!("live radio: unreadable now-playing answer"),
            Err(error) => {
                log::debug!(
                    "live radio: now playing unavailable: {}",
                    error.without_url()
                );
            }
        }
        if wait(stop, META_INTERVAL) {
            return;
        }
    }
}

fn clean(text: Option<String>) -> String {
    text.map(|text| text.trim().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_now_playing_answer_reads_through_its_byte_order_mark() {
        let body = "\u{feff}{\"id\":null,\"image\":null,\"current_artist\":\"LOUNGE MIX\",\"current_song\":\" \",\"next_song\":{\"id\":\"1\"}}";
        let meta = parse_meta(body).expect("parsed");
        assert_eq!(clean(meta.current_artist), "LOUNGE MIX");
        assert_eq!(clean(meta.current_song), "");
        assert!(meta.image.is_none());
    }

    #[test]
    fn the_bar_names_the_song_or_else_the_show() {
        let mut state = RadioState {
            artist: "Dua Lipa".into(),
            song: "Houdini".into(),
            ..RadioState::default()
        };
        assert_eq!(
            state.title_and_byline(&RADIO_NOVA),
            ("Houdini".into(), "Dua Lipa \u{b7} Radio Nova".into())
        );
        state.song.clear();
        state.artist = "LOUNGE MIX".into();
        assert_eq!(
            state.title_and_byline(&RADIO_NOVA),
            ("LOUNGE MIX".into(), "Radio Nova".into())
        );
        state.artist.clear();
        assert_eq!(
            state.title_and_byline(&RADIO_NOVA),
            ("Radio Nova".into(), String::new())
        );
    }

    /// Plays two seconds of the real station into a counting sink.
    /// Needs the network: `cargo test --lib live_radio -- --ignored`.
    #[test]
    #[ignore = "reaches the station over the network"]
    fn the_station_stream_decodes_to_spotify_stereo() {
        struct Counting {
            samples: usize,
            stop: Arc<AtomicBool>,
        }
        impl Sink for Counting {
            fn write(
                &mut self,
                packet: AudioPacket,
                _: &mut Converter,
            ) -> librespot_playback::audio_backend::SinkResult<()> {
                self.samples += packet.samples().unwrap().len();
                if self.samples >= 2 * SAMPLE_RATE as usize * NUM_CHANNELS as usize {
                    self.stop.store(true, Ordering::Relaxed);
                }
                Ok(())
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let mut sink = Counting {
            samples: 0,
            stop: Arc::clone(&stop),
        };
        let http = crate::http::build_blocking(
            &crate::settings::ProxyConfig::System,
            Duration::from_secs(15),
        )
        .unwrap();
        let state = Mutex::new(RadioState::default());
        let mut started = false;
        play_once(
            &RADIO_NOVA,
            &http,
            &stop,
            &mut sink,
            &mut started,
            &state,
            &Waker::default(),
        )
        .unwrap();
        assert!(started && stop.load(Ordering::Relaxed));
        let mut meta = String::new();
        std::io::Read::read_to_string(&mut http.get(RADIO_NOVA.meta).send().unwrap(), &mut meta)
            .unwrap();
        assert!(parse_meta(&meta).is_some(), "{meta}");
    }

    #[test]
    fn mono_and_other_rates_reach_the_output_as_spotify_stereo() {
        let mut stereo = Stereo::default();
        assert_eq!(
            stereo.convert(&[0.5, -0.5], 1, SAMPLE_RATE),
            vec![0.5, 0.5, -0.5, -0.5]
        );
        let frames = 4_800;
        let resampled = stereo.convert(&vec![0.0; frames * 2], 2, 48_000);
        let expected = frames * SAMPLE_RATE as usize / 48_000 * 2;
        assert!(
            resampled.len().abs_diff(expected) <= 64,
            "{} vs {expected}",
            resampled.len()
        );
    }
}
