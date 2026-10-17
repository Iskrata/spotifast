//! Live internet radio: one station's stream, played through Spotifast's own
//! output so the equalizer and every visualiser follow it as they follow
//! Spotify. It never plays at the same time as Spotify: starting the radio
//! pauses Spotify, and playing anything from Spotify leaves the radio.
//!
//! The station broadcasts through Triton Digital, whose stream comes with a
//! sideband of cue points: each song as it starts, and each advert. The cues
//! name the song on air, and an advert cue mutes the radio until the next
//! song's cue. News and talk carry no cues and stay audible. Every cue is
//! stamped with its place in this connection's stream, so it is applied
//! when the audio around it reaches the speakers, not when it arrives.

use std::collections::VecDeque;
use std::io::BufRead;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

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
use crate::settings::ProxyConfig;

/// A station Spotifast can play: a Triton Digital mount with cue points.
#[derive(Debug, PartialEq, Eq)]
pub struct Station {
    pub name: &'static str,
    /// The Triton mount; its stream with cues is `{mount}_SC` and the cues
    /// themselves are `{mount}_SBM`, on the server the stream lands on.
    pub mount: &'static str,
    /// Where the stream's address is asked for; tests point it elsewhere.
    pub directory: &'static str,
    pub logo: &'static str,
}

/// Radio Nova, Sofia: its website's 128 kbps AAC-LC stream.
pub const RADIO_NOVA: Station = Station {
    name: "Radio Nova",
    mount: "RADIO_NOVAAAC_H",
    directory: "https://playerservices.streamtheworld.com/api/livestream-redirect/",
    logo: "https://media.metacast.eu/pictures/05_2021/Ws5JjPdSiBDnfyVE70Hq-67892.png",
};

/// The song on air when a stream connects, before its first cue.
const NOW_PLAYING: &str = "https://np.tritondigital.com/public/nowplaying";
/// How long the stream may send nothing before it is reopened.
const STREAM_TIMEOUT: Duration = Duration::from_secs(15);
/// How long the cues may be quiet before their connection is reopened. News
/// carries none, and a long song only two, so this is generous.
const CUE_TIMEOUT: Duration = Duration::from_secs(600);
/// Longer than any advert break: a break whose closing cue went missing is
/// not left muted.
const MAX_AD_BREAK: Duration = Duration::from_secs(8 * 60);
/// About how much decoded sound waits in the output's queue (twelve chunks,
/// see `sink::QUEUE_LIMIT`) ahead of the device's own buffer.
const QUEUE_LAG_MS: u64 = 250;
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
    /// An advert break is on, and the radio is muted until it ends.
    pub ad_break: bool,
}

impl RadioState {
    /// The bar's title and byline: the song and its artist, the muted
    /// break as `ad_break`, or the station alone.
    pub fn title_and_byline(&self, station: &Station, ad_break: &str) -> (String, String) {
        if self.ad_break {
            return (ad_break.to_string(), station.name.to_string());
        }
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

/// The radio's volume, on Spotify's 0 to 65535 scale and curve, silent
/// through an advert break. The output ramps the change, so a break starts
/// and ends without a click, and the visualisers, fed before the volume,
/// keep moving.
#[derive(Clone)]
struct Volume {
    level: Arc<AtomicU16>,
    muted: Arc<AtomicBool>,
}

impl VolumeGetter for Volume {
    fn attenuation_factor(&self) -> f64 {
        if self.muted.load(Ordering::Relaxed) {
            return 0.0;
        }
        crate::player::VOLUME_CTRL.to_mapped(self.level.load(Ordering::Relaxed))
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
            volume: Volume {
                level: Arc::new(AtomicU16::new(volume)),
                muted: Arc::default(),
            },
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
        self.volume.level.load(Ordering::Relaxed)
    }

    pub fn set_volume(&self, volume: u16) {
        self.volume.level.store(volume, Ordering::Relaxed);
    }

    /// Starts the stream through `proxy`.
    pub fn play(&mut self, proxy: &ProxyConfig) -> Result<(), String> {
        let stream_http = crate::http::build_blocking(proxy, STREAM_TIMEOUT)?;
        let cue_http = crate::http::build_blocking(proxy, CUE_TIMEOUT)?;
        self.end_stream();
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.on = true;
            state.playing = true;
            state.loading = true;
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Some(Arc::clone(&stop));
        let radio = Radio {
            station: self.station,
            stream_http,
            cue_http,
            stop,
            state: Arc::clone(&self.state),
            muted: Arc::clone(&self.volume.muted),
            lag_ms: u64::from(self.output.buffer_ms) + QUEUE_LAG_MS,
            waker: self.waker.clone(),
        };
        let output = self.output();
        thread::Builder::new()
            .name("live-radio".into())
            .spawn(move || {
                // librespot's sinks stay on the thread that made them.
                let sink = crate::player::own_output(output);
                radio.run(sink);
            })
            .map(|_| ())
            .map_err(|error| format!("could not start the radio: {error}"))
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
        self.volume.muted.store(false, Ordering::Relaxed);
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ad_break = false;
    }

    fn output(&self) -> crate::player::OwnOutput {
        let report: crate::sink::ErrorHook = Arc::new(|message: String| {
            log::warn!("live radio output: {message}");
        });
        crate::player::OwnOutput {
            device: self.output.device.clone(),
            buffer_ms: self.output.buffer_ms,
            report,
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

/// A cue point: what the station puts on air.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Cue {
    Song { artist: String, title: String },
    Advert,
}

/// Cues waiting for the audio they mark to be heard, by their place in the
/// stream in milliseconds.
type Pending = Arc<Mutex<VecDeque<(u64, Cue)>>>;

/// One play of the radio, owned by its stream thread.
struct Radio {
    station: &'static Station,
    stream_http: reqwest::blocking::Client,
    cue_http: reqwest::blocking::Client,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<RadioState>>,
    muted: Arc<AtomicBool>,
    /// How long decoded sound takes to reach the speakers.
    lag_ms: u64,
    waker: Waker,
}

impl Radio {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn update(&self, change: impl FnOnce(&mut RadioState)) {
        // A stream told to stop no longer speaks for the radio: a newer one may.
        if self.stopped() {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let before = state.clone();
        change(&mut state);
        if *state != before {
            drop(state);
            self.waker.wake();
        }
    }

    fn apply(&self, cue: Cue) {
        if self.stopped() {
            return;
        }
        let advert = cue == Cue::Advert;
        if advert != self.muted.swap(advert, Ordering::Relaxed) {
            log::info!(
                "live radio: {}",
                if advert {
                    "advert break, muted"
                } else {
                    "advert break over"
                }
            );
        }
        self.update(|state| {
            state.ad_break = advert;
            if let Cue::Song { artist, title } = cue {
                state.artist = artist;
                state.song = title;
            }
        });
    }

    /// Plays the station until stopped, reconnecting when the stream drops.
    fn run(self, mut sink: Box<dyn Sink>) {
        let mut started = false;
        while !self.stopped() {
            let result = self.play_once(sink.as_mut(), &mut started);
            if self.stopped() {
                break;
            }
            match result {
                Ok(()) => log::info!("live radio: the stream ended; reconnecting"),
                Err(error) => log::warn!("live radio: {error}; reconnecting"),
            }
            self.update(|state| state.loading = true);
            if wait(&self.stop, RETRY_AFTER) {
                break;
            }
        }
        if started && let Err(error) = sink.stop() {
            log::warn!("live radio: could not stop the output: {error}");
        }
    }

    fn play_once(&self, sink: &mut dyn Sink, started: &mut bool) -> Result<(), String> {
        let id = sideband_id();
        let response = self
            .stream_http
            .get(format!(
                "{}{}_SC",
                self.station.directory, self.station.mount
            ))
            .query(&[("sbmid", id.as_str())])
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|error| format!("could not open the stream: {}", error.without_url()))?;
        // The cues come from the server the stream landed on, for this id.
        let mut cues_url = response.url().clone();
        cues_url.set_path(&format!("/{}_SBM", self.station.mount));
        let pending = Pending::default();
        let connection = Arc::new(AtomicBool::new(false));
        let _closing = SetOnDrop(Arc::clone(&connection));
        self.spawn_cues(cues_url, Arc::clone(&pending), Arc::clone(&connection));

        let source =
            MediaSourceStream::new(Box::new(ReadOnlySource::new(response)), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("aac");
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
        // Milliseconds of this connection's stream decoded, which is how
        // its cues are stamped.
        let mut decoded_ms = 0.0f64;
        let mut muted_since: Option<Instant> = None;
        while !self.stopped() {
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
                // A damaged frame is skipped, as every player does.
                Err(DecodeError::DecodeError(error)) => {
                    log::debug!("live radio: skipped a frame: {error}");
                    continue;
                }
                Err(error) => return Err(format!("decoding failed: {error}")),
            };
            let spec = *decoded.spec();
            decoded_ms += decoded.frames() as f64 * 1000.0 / f64::from(spec.rate.max(1));
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
            self.update(|state| state.loading = false);
            // The output applies volume to what it has queued, so a cue
            // waits until its own sound is about to be heard.
            let heard_ms = (decoded_ms as u64).saturating_sub(self.lag_ms);
            for cue in due(&pending, heard_ms) {
                self.apply(cue);
            }
            if self.muted.load(Ordering::Relaxed) {
                let since = *muted_since.get_or_insert_with(Instant::now);
                if since.elapsed() > MAX_AD_BREAK {
                    log::warn!("live radio: no cue ended the advert break; unmuting");
                    self.muted.store(false, Ordering::Relaxed);
                    self.update(|state| state.ad_break = false);
                }
            } else {
                muted_since = None;
            }
        }
        Ok(())
    }

    /// Reads this connection's cues until it closes, and first the song
    /// already on air.
    fn spawn_cues(&self, url: reqwest::Url, pending: Pending, connection: Arc<AtomicBool>) {
        let http = self.cue_http.clone();
        let mount = self.station.mount;
        let (stop, state, waker) = (
            Arc::clone(&self.stop),
            Arc::clone(&self.state),
            self.waker.clone(),
        );
        let spawned = thread::Builder::new()
            .name("live-radio-cues".into())
            .spawn(move || {
                let closed = || stop.load(Ordering::Relaxed) || connection.load(Ordering::Relaxed);
                if let Some(Cue::Song { artist, title }) = now_playing(&http, mount)
                    && !closed()
                {
                    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                    if state.song.is_empty() {
                        state.artist = artist;
                        state.song = title;
                        drop(state);
                        waker.wake();
                    }
                }
                while !closed() {
                    if let Err(error) = read_cues(&http, &url, &pending, &closed) {
                        log::debug!("live radio: cues interrupted: {error}");
                    }
                    if closed() || wait(&connection, RETRY_AFTER) {
                        return;
                    }
                }
            });
        if let Err(error) = spawned {
            log::warn!("live radio: could not read its cues: {error}");
        }
    }
}

/// Sets its flag when dropped: the stream connection has closed.
struct SetOnDrop(Arc<AtomicBool>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Takes the cues whose place in the stream has been reached, in order.
fn due(pending: &Mutex<VecDeque<(u64, Cue)>>, heard_ms: u64) -> Vec<Cue> {
    let mut pending = pending.lock().unwrap_or_else(PoisonError::into_inner);
    let mut due = Vec::new();
    while pending.front().is_some_and(|(at, _)| *at <= heard_ms) {
        if let Some((_, cue)) = pending.pop_front() {
            due.push(cue);
        }
    }
    due
}

/// A fresh sideband id in UUID form, as Triton's own players send.
fn sideband_id() -> String {
    let bytes: [u8; 16] = rand::random();
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-4{}-a{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

/// One server-sent event of the sideband.
#[derive(Debug, Deserialize)]
struct CueEvent {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    /// The cue's place in this connection's stream, in milliseconds.
    timestamp: u64,
    #[serde(default)]
    parameters: CueParameters,
}

#[derive(Debug, Default, Deserialize)]
struct CueParameters {
    #[serde(default)]
    cue_title: String,
    #[serde(default)]
    track_artist_name: String,
}

/// The cue an event announces, with its place in the stream.
fn parse_cue(data: &str) -> Option<(u64, Cue)> {
    let event: CueEvent = serde_json::from_str(data).ok()?;
    if event.kind != "onCuePoint" {
        return None;
    }
    let cue = match event.name.as_str() {
        "ad" => Cue::Advert,
        "track" => Cue::Song {
            artist: unescape(&event.parameters.track_artist_name),
            title: unescape(&event.parameters.cue_title),
        },
        _ => return None,
    };
    Some((event.timestamp, cue))
}

/// Reads server-sent events from `url` until the connection ends.
fn read_cues(
    http: &reqwest::blocking::Client,
    url: &reqwest::Url,
    pending: &Mutex<VecDeque<(u64, Cue)>>,
    closed: &dyn Fn() -> bool,
) -> Result<(), String> {
    let response = http
        .get(url.clone())
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| error.without_url().to_string())?;
    let mut data = String::new();
    for line in std::io::BufReader::new(response).lines() {
        let line = line.map_err(|error| error.to_string())?;
        if closed() {
            return Ok(());
        }
        if let Some(part) = line.strip_prefix("data:") {
            data.push_str(part);
            data.push('\n');
        } else if line.trim().is_empty() && !data.is_empty() {
            if let Some(cue) = parse_cue(&data) {
                log::debug!("live radio: cue {cue:?}");
                pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push_back(cue);
            }
            data.clear();
        }
    }
    Ok(())
}

/// The song Triton last announced on `mount`.
fn now_playing(http: &reqwest::blocking::Client, mount: &str) -> Option<Cue> {
    let body = http
        .get(NOW_PLAYING)
        .query(&[
            ("mountName", mount),
            ("numberToFetch", "1"),
            ("eventType", "track"),
        ])
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::text)
        .ok()?;
    let title = xml_property(&body, "cue_title")?;
    Some(Cue::Song {
        artist: xml_property(&body, "track_artist_name").unwrap_or_default(),
        title,
    })
}

/// A `<property name="…"><![CDATA[…]]></property>` of Triton's now-playing
/// answer: the only shape it uses, so no XML parser is needed.
fn xml_property(body: &str, name: &str) -> Option<String> {
    let start = body.find(&format!("<property name=\"{name}\"><![CDATA["))?;
    let rest = &body[start..];
    let rest = &rest[rest.find("<![CDATA[")? + "<![CDATA[".len()..];
    let value = unescape(rest[..rest.find("]]>")?].trim());
    (!value.is_empty()).then_some(value)
}

/// Undoes the HTML escapes Triton leaves in names, as in `Chus &amp; Ceballos`.
fn unescape(text: &str) -> String {
    text.trim()
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Two events as Triton's sideband sent them, each `data:` line of the
    /// JSON joined as `read_cues` joins them.
    const ADVERT: &str = "{\n\t\"type\" : \"onCuePoint\",\n\t\"name\" : \"ad\",\n\t\"timestamp\" : 2043658,\n\t\"parameters\" : {\n\t\t\"ad_count\" : \"1\",\n\t\t\"ad_type\" : \"insert\",\n\t\t\"cue_id\" : \"27a53978-ddff-4c0d-ad69-5a3abe77e2a8\",\n\t\t\"cue_time_duration\" : \"4.9633\",\n\t\t\"cue_time_start\" : \"1792145907857\",\n\t\t\"cue_title\" : \"AD_INSERTION\"\n\t}\n}\n";
    const SONG: &str = "{\n\t\"type\" : \"onCuePoint\",\n\t\"name\" : \"track\",\n\t\"timestamp\" : 1620451,\n\t\"parameters\" : {\n\t\t\"cue_id\" : \"e2bfaa06-4915-47e9-857d-832a76dbedb0\",\n\t\t\"cue_time_duration\" : \"227.3176\",\n\t\t\"cue_time_start\" : \"1792145484562\",\n\t\t\"cue_title\" : \"All I Want\",\n\t\t\"program_id\" : \"F3139\",\n\t\t\"track_artist_name\" : \"Chus &amp; Ceballos, Astrid Suryanto\"\n\t}\n}\n";

    #[test]
    fn sideband_cues_name_songs_and_adverts_at_their_place_in_the_stream() {
        assert_eq!(parse_cue(ADVERT), Some((2_043_658, Cue::Advert)));
        assert_eq!(
            parse_cue(SONG),
            Some((
                1_620_451,
                Cue::Song {
                    artist: "Chus & Ceballos, Astrid Suryanto".into(),
                    title: "All I Want".into(),
                }
            ))
        );
        assert_eq!(
            parse_cue("{\"type\":\"onMetaData\",\"name\":\"x\",\"timestamp\":1}"),
            None
        );
    }

    #[test]
    fn a_cue_waits_until_its_sound_is_heard() {
        let pending = Pending::default();
        pending.lock().unwrap().extend([
            (1_000, Cue::Advert),
            (
                5_000,
                Cue::Song {
                    artist: String::new(),
                    title: "Next".into(),
                },
            ),
        ]);
        assert!(due(&pending, 999).is_empty());
        assert_eq!(due(&pending, 1_000), vec![Cue::Advert]);
        assert!(due(&pending, 4_999).is_empty());
        assert_eq!(due(&pending, 9_000).len(), 1);
        assert!(pending.lock().unwrap().is_empty());
    }

    #[test]
    fn an_advert_break_silences_the_output_and_the_next_song_restores_it() {
        let state = Arc::new(Mutex::new(RadioState {
            on: true,
            playing: true,
            ..RadioState::default()
        }));
        let volume = Volume {
            level: Arc::new(AtomicU16::new(u16::MAX)),
            muted: Arc::default(),
        };
        let radio = Radio {
            station: &RADIO_NOVA,
            stream_http: reqwest::blocking::Client::new(),
            cue_http: reqwest::blocking::Client::new(),
            stop: Arc::default(),
            state: Arc::clone(&state),
            muted: Arc::clone(&volume.muted),
            lag_ms: 0,
            waker: Waker::default(),
        };
        assert!(volume.attenuation_factor() > 0.99);
        radio.apply(Cue::Advert);
        assert_eq!(volume.attenuation_factor(), 0.0);
        assert!(state.lock().unwrap().ad_break);
        radio.apply(parse_cue(SONG).unwrap().1);
        assert!(volume.attenuation_factor() > 0.99);
        let state = state.lock().unwrap();
        assert!(!state.ad_break);
        assert_eq!(state.song, "All I Want");
    }

    #[test]
    fn the_song_on_air_reads_from_tritons_now_playing_answer() {
        let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><nowplaying-info-list><nowplaying-info mountName=\"RADIO_NOVAAAC_H\" timestamp=\"1792143710\" type=\"track\"><property name=\"cue_time_duration\"><![CDATA[378.6188]]></property><property name=\"cue_title\"><![CDATA[Can't Feel My Face (TYLA Remix)]]></property><property name=\"track_artist_name\"><![CDATA[THE WEEKND]]></property></nowplaying-info></nowplaying-info-list>";
        assert_eq!(
            xml_property(body, "cue_title").as_deref(),
            Some("Can't Feel My Face (TYLA Remix)")
        );
        assert_eq!(
            xml_property(body, "track_artist_name").as_deref(),
            Some("THE WEEKND")
        );
        assert_eq!(xml_property(body, "program_id"), None);
    }

    #[test]
    fn the_bar_names_the_song_the_break_or_the_station() {
        let mut state = RadioState {
            artist: "Seeb, Neev".into(),
            song: "Breathe".into(),
            ..RadioState::default()
        };
        assert_eq!(
            state.title_and_byline(&RADIO_NOVA, "Ad break"),
            ("Breathe".into(), "Seeb, Neev \u{b7} Radio Nova".into())
        );
        state.ad_break = true;
        assert_eq!(
            state.title_and_byline(&RADIO_NOVA, "Ad break"),
            ("Ad break".into(), "Radio Nova".into())
        );
        let state = RadioState::default();
        assert_eq!(
            state.title_and_byline(&RADIO_NOVA, "Ad break"),
            ("Radio Nova".into(), String::new())
        );
    }

    #[test]
    fn a_sideband_id_has_the_uuid_form() {
        let id = sideband_id();
        let parts: Vec<_> = id.split('-').map(str::len).collect();
        assert_eq!(parts, [8, 4, 4, 4, 12]);
        assert_ne!(id, sideband_id());
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
        let http = |timeout| crate::http::build_blocking(&ProxyConfig::System, timeout).unwrap();
        let state = Arc::new(Mutex::new(RadioState::default()));
        let radio = Radio {
            station: &RADIO_NOVA,
            stream_http: http(STREAM_TIMEOUT),
            cue_http: http(CUE_TIMEOUT),
            stop: Arc::default(),
            state: Arc::clone(&state),
            muted: Arc::default(),
            lag_ms: 0,
            waker: Waker::default(),
        };
        let mut sink = Counting {
            samples: 0,
            stop: Arc::clone(&radio.stop),
        };
        let mut started = false;
        radio.play_once(&mut sink, &mut started).unwrap();
        assert!(started && radio.stopped());
        assert!(now_playing(&radio.cue_http, RADIO_NOVA.mount).is_some());
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
