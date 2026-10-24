//! Music for the live radio's advert breaks: Spotify's song radio for the
//! song the station played last, played by a private librespot player whose
//! sound goes into the radio's break mix instead of to an output. The player
//! is no Connect device and has no queue the interface shows; only the
//! radio starts and stops it. Without a Spotify session, or without a match
//! for the station's song, a break is silent.
//!
//! The radio names each song it puts on air ([`BreakMusic::seed`]); a task
//! on the runtime finds the song on Spotify, resolves its radio, and loads
//! the first of its songs paused, so a break starts with music at once.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use librespot_core::{Session, SpotifyUri};
use librespot_playback::audio_backend::{Sink, SinkResult};
use librespot_playback::config::PlayerConfig;
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::NoOpVolume;
use librespot_playback::player::{Player, PlayerEvent};
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};
use tokio::sync::watch;

use crate::api::{ApiGateway, Operation};

/// How much decoded music waits for the radio to take it: half a second.
const FEED_CAPACITY: usize = SAMPLE_RATE as usize * NUM_CHANNELS as usize / 2;

/// The song a break's music is chosen by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seed {
    pub artist: String,
    pub title: String,
}

/// The break music shared by the radio's stream thread, the Spotify engine
/// that lends it a session, and the task that chooses its songs.
pub struct BreakMusic {
    feed: Arc<Feed>,
    inner: Mutex<Inner>,
    seeds: watch::Sender<Option<Seed>>,
}

#[derive(Default)]
struct Inner {
    /// The player on the engine's session, while there is one.
    player: Option<Attached>,
    /// The songs to play next, the first of them loaded already.
    songs: VecDeque<SpotifyUri>,
}

struct Attached {
    player: Arc<Player>,
    session: Session,
    /// Which engine attached it, so a replaced engine's shutdown does not
    /// take its successor's player.
    owner: u64,
    /// Whether a song is loaded, paused or playing.
    loaded: bool,
}

impl std::fmt::Debug for BreakMusic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreakMusic")
            .field("seed", &*self.seeds.borrow())
            .finish_non_exhaustive()
    }
}

impl Default for BreakMusic {
    fn default() -> Self {
        Self {
            feed: Arc::default(),
            inner: Mutex::default(),
            seeds: watch::Sender::new(None),
        }
    }
}

impl BreakMusic {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Gives the music a player on `session`, for the engine `owner`. Must
    /// be called on the runtime: the player's events are read there.
    pub fn attach(self: &Arc<Self>, session: &Session, owner: u64) {
        let feed = Arc::clone(&self.feed);
        let player = Player::new(
            PlayerConfig::default(),
            session.clone(),
            Box::new(NoOpVolume),
            move || Box::new(FeedSink(feed)),
        );
        let mut events = player.get_player_event_channel();
        let music = Arc::downgrade(self);
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let Some(music) = music.upgrade() else { break };
                match event {
                    PlayerEvent::EndOfTrack { .. } | PlayerEvent::Unavailable { .. } => {
                        music.next_song(owner);
                    }
                    _ => {}
                }
            }
        });
        let mut inner = self.inner();
        if let Some(old) = inner.player.take() {
            old.player.stop();
        }
        inner.player = Some(Attached {
            player,
            session: session.clone(),
            owner,
            loaded: false,
        });
        inner.load_first(self.feed.is_open());
        drop(inner);
        // The seed may have come before any session could resolve it.
        self.seeds.send_modify(|_| {});
    }

    /// Takes the player away when the engine `owner` shuts down.
    pub fn detach(&self, owner: u64) {
        let mut inner = self.inner();
        if inner
            .player
            .as_ref()
            .is_some_and(|player| player.owner == owner)
            && let Some(old) = inner.player.take()
        {
            old.player.stop();
        }
    }

    /// The radio put `seed` on air: choose the next break's music by it.
    pub fn seed(&self, seed: Seed) {
        self.seeds.send_if_modified(|current| {
            let changed = current.as_ref() != Some(&seed);
            *current = Some(seed);
            changed
        });
    }

    /// Plays the next break with `songs`. A song already playing through a
    /// break plays on, and these follow it.
    fn prepare(&self, songs: Vec<SpotifyUri>) {
        let mut inner = self.inner();
        inner.songs = songs.into();
        let playing = self.feed.is_open() && inner.player.as_ref().is_some_and(|p| p.loaded);
        if !playing {
            inner.load_first(self.feed.is_open());
        }
    }

    /// A song ended or could not play: the next one plays in its place.
    fn next_song(&self, owner: u64) {
        let mut inner = self.inner();
        if inner
            .player
            .as_ref()
            .is_none_or(|player| player.owner != owner)
        {
            return;
        }
        inner.songs.pop_front();
        inner.load_first(self.feed.is_open());
    }

    /// A break started: the music plays into the feed.
    pub fn start(&self) {
        self.feed.open();
        if let Some(attached) = &self.inner().player {
            attached.player.play();
        }
    }

    /// The break is over: the music pauses, and what it had sent is
    /// dropped, so the next break does not start with this one's tail.
    pub fn stop(&self) {
        // Closed first, so a player waiting on a full feed is let go and
        // reads the pause.
        self.feed.close();
        if let Some(attached) = &self.inner().player {
            attached.player.pause();
        }
    }

    /// Fills `out` with the music's next interleaved stereo samples, and
    /// with silence where it has none.
    pub fn take(&self, out: &mut [f64]) {
        self.feed.take(out);
    }

    fn session(&self) -> Option<Session> {
        self.inner()
            .player
            .as_ref()
            .map(|attached| attached.session.clone())
    }

    /// Chooses each seed's songs, the newest seed first, for as long as the
    /// music is shared. Runs on the runtime.
    pub async fn choose_songs(self: Arc<Self>, api: Arc<ApiGateway>) {
        let mut seeds = self.seeds.subscribe();
        let music = Arc::downgrade(&self);
        drop(self);
        while seeds.changed().await.is_ok() {
            let Some(seed) = seeds.borrow_and_update().clone() else {
                continue;
            };
            let Some(session) = music.upgrade().and_then(|music| music.session()) else {
                continue;
            };
            match similar_songs(&api, &session, &seed).await {
                Ok(songs) => {
                    log::info!(
                        "break music: {} songs like {} by {}",
                        songs.len(),
                        seed.title,
                        seed.artist
                    );
                    if let Some(music) = music.upgrade() {
                        music.prepare(songs);
                    }
                }
                Err(error) => log::info!(
                    "break music: nothing like {} by {}: {error:#}",
                    seed.title,
                    seed.artist
                ),
            }
        }
    }
}

impl Inner {
    /// Loads the first of the songs, playing at once when `play`.
    fn load_first(&mut self, play: bool) {
        let Some(attached) = self.player.as_mut() else {
            return;
        };
        match self.songs.front() {
            Some(song) => {
                attached.player.load(song.clone(), play, 0);
                attached.loaded = true;
            }
            None => {
                attached.player.stop();
                attached.loaded = false;
            }
        }
    }
}

/// The songs of Spotify's radio for the station's song, or for its artist
/// when Spotify does not have the song.
async fn similar_songs(
    api: &ApiGateway,
    session: &Session,
    seed: &Seed,
) -> anyhow::Result<Vec<SpotifyUri>> {
    let client = api.client_for(Operation::CatalogSearch).await?;
    let query = format!("track:{} artist:{}", seed.title, seed.artist);
    let found = client.search(&query, &["track"]).await?;
    let mut radio_seed = found
        .tracks
        .and_then(|tracks| tracks.items.into_iter().next())
        .map(|track| track.uri);
    if radio_seed.is_none() {
        let found = client.search(&seed.artist, &["artist"]).await?;
        radio_seed = found
            .artists
            .and_then(|artists| artists.items.into_iter().next())
            .map(|artist| artist.uri);
    }
    let station = radio_seed
        .as_deref()
        .and_then(crate::util::station_uri)
        .ok_or_else(|| anyhow::anyhow!("Spotify has neither the song nor the artist"))?;
    let songs: Vec<SpotifyUri> = crate::session_reads::station(session, &station)
        .await?
        .iter()
        .filter_map(|track| SpotifyUri::from_uri(&track.uri).ok())
        .collect();
    anyhow::ensure!(!songs.is_empty(), "{station} has no playable songs");
    Ok(songs)
}

/// Decoded music on its way from the player's thread to the radio's.
#[derive(Default)]
struct Feed {
    state: Mutex<FeedState>,
    /// Signalled when samples are taken or the feed closes.
    space: Condvar,
}

#[derive(Default)]
struct FeedState {
    /// Through a break; a closed feed drops what it is sent.
    open: bool,
    samples: VecDeque<f64>,
}

impl Feed {
    fn state(&self) -> std::sync::MutexGuard<'_, FeedState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_open(&self) -> bool {
        self.state().open
    }

    fn open(&self) {
        self.state().open = true;
    }

    fn close(&self) {
        let mut state = self.state();
        state.open = false;
        state.samples.clear();
        drop(state);
        self.space.notify_all();
    }

    /// Waits for room while the feed is open, so the player keeps pace
    /// with the radio.
    fn put(&self, samples: &[f64]) {
        let mut state = self.state();
        while state.open && state.samples.len() >= FEED_CAPACITY {
            state = self
                .space
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if state.open {
            state.samples.extend(samples);
        }
    }

    fn take(&self, out: &mut [f64]) {
        let mut state = self.state();
        let ready = out.len().min(state.samples.len());
        for (slot, sample) in out.iter_mut().zip(state.samples.drain(..ready)) {
            *slot = sample;
        }
        drop(state);
        out[ready..].fill(0.0);
        self.space.notify_all();
    }
}

/// The private player's output: the feed.
struct FeedSink(Arc<Feed>);

impl Sink for FeedSink {
    fn write(&mut self, packet: AudioPacket, _: &mut Converter) -> SinkResult<()> {
        if let AudioPacket::Samples(samples) = packet {
            self.0.put(&samples);
        }
        Ok(())
    }
}

#[cfg(test)]
impl BreakMusic {
    /// Opens the feed and sends it `samples`, as the player would.
    pub(crate) fn play_samples(&self, samples: &[f64]) {
        self.feed.open();
        self.feed.put(samples);
    }

    /// The seed the next break's music is chosen by.
    pub(crate) fn seeded(&self) -> Option<Seed> {
        self.seeds.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feed_hands_over_music_then_silence() {
        let music = BreakMusic::default();
        music.play_samples(&[0.5, -0.5]);
        let mut out = [1.0; 4];
        music.take(&mut out);
        assert_eq!(out, [0.5, -0.5, 0.0, 0.0]);
    }

    #[test]
    fn a_closed_feed_drops_what_it_is_sent() {
        let music = BreakMusic::default();
        music.play_samples(&[0.5, 0.5]);
        music.stop();
        music.feed.put(&[0.25, 0.25]);
        let mut out = [1.0; 2];
        music.take(&mut out);
        assert_eq!(out, [0.0, 0.0]);
    }

    #[test]
    fn a_player_waiting_on_a_full_feed_is_let_go_when_the_break_ends() {
        let music = Arc::new(BreakMusic::default());
        music.play_samples(&vec![0.0; FEED_CAPACITY]);
        let writer = {
            let music = Arc::clone(&music);
            std::thread::spawn(move || music.feed.put(&[0.0; 2]))
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!writer.is_finished());
        music.stop();
        writer.join().unwrap();
    }

    #[test]
    fn a_seed_is_sent_once_until_the_song_changes() {
        let music = BreakMusic::default();
        let mut seeds = music.seeds.subscribe();
        let seed = Seed {
            artist: "Astrid Suryanto".into(),
            title: "All I Want".into(),
        };
        music.seed(seed.clone());
        assert!(seeds.has_changed().unwrap());
        seeds.mark_unchanged();
        music.seed(seed);
        assert!(!seeds.has_changed().unwrap());
    }
}
