//! Diagnostic: the biography and portraits the playback session reads for
//! an artist, as the now playing panel's About the artist card shows them.
//! Read-only.
//!
//!   cargo run --example artist_probe -- <artist id>

use librespot_core::{Session, SessionConfig, cache::Cache};

fn main() -> anyhow::Result<()> {
    let Some(id) = std::env::args().nth(1) else {
        anyhow::bail!("usage: artist_probe <artist id>");
    };
    fastframe_log::Logging::new("spotifast", env!("CARGO_PKG_VERSION"))
        .filter("warn")
        .init()?;

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

        let profile = spotifast::artist_profile::read(&session, &id).await?;
        match &profile.biography {
            Some(biography) => println!(
                "biography ({} characters):\n{biography}",
                biography.chars().count()
            ),
            None => println!("Spotify has no biography for this artist"),
        }
        println!("{} portraits", profile.portraits.len());
        for portrait in &profile.portraits {
            println!(
                "  {} ({:?}x{:?})",
                portrait.url, portrait.width, portrait.height
            );
        }
        anyhow::Ok(())
    })?;
    Ok(())
}
