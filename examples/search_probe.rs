//! Diagnostic: the playlists a search finds as Spotifast reads them over
//! the local playback session, with the stored playback credential. This is
//! the read that stands in for the playlist half of a search on the shared
//! Web API app. Nothing is written.
//!
//!   cargo run --example search_probe -- "daily mix"

use librespot_core::{Session, SessionConfig, cache::Cache};

fn main() -> anyhow::Result<()> {
    fastframe_log::Logging::new("spotifast", env!("CARGO_PKG_VERSION"))
        .filter("warn")
        .init()?;

    let query = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Discover Weekly".into());
    let dirs = spotifast::paths::AppDirs::discover();
    let cache = Cache::new::<&std::path::Path>(None, None, None, None)?.with_memory_credentials();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let store = spotifast::credentials::Store::new(dirs);
        let loaded = store
            .lease(spotifast::credentials::Slot::Playback)
            .load()
            .await?;
        if let Some(warning) = loaded.warning {
            eprintln!("{warning}");
        }
        let Some(spotifast::credentials::Grant::Playback(credentials)) = loaded.grant else {
            anyhow::bail!("Enable playback in Spotifast first");
        };
        let session = Session::new(SessionConfig::default(), Some(cache));
        session.connect(credentials, false).await?;
        println!("connected as {}", session.username());

        let started = std::time::Instant::now();
        let page = match spotifast::session_search::playlists(&session, &query).await {
            Ok(page) => page,
            Err(spotifast::session_reads::Failure::Definitive(error)) => {
                anyhow::bail!("refused: {error}")
            }
            Err(spotifast::session_reads::Failure::Retry(error)) => {
                anyhow::bail!("unavailable: {error:#}")
            }
        };
        let elapsed = started.elapsed();
        let spotify_owned = page
            .items
            .iter()
            .filter(|playlist| playlist.owner.id.as_deref() == Some("spotify"))
            .count();
        println!(
            "{} of {} playlists for {query:?} in {elapsed:?}: {spotify_owned} owned by Spotify",
            page.items.len(),
            page.total,
        );
        for playlist in &page.items {
            println!(
                "  {:<24} {:<40} owner={:<26} cover={}",
                playlist.id,
                playlist.name,
                playlist.owner_name(),
                playlist
                    .images
                    .first()
                    .map_or("none", |image| image.url.as_str()),
            );
        }
        anyhow::Ok(())
    })?;
    Ok(())
}
