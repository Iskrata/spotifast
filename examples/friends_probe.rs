//! Diagnostic: whether Spotify answers the buddy list (Friend Activity) for
//! the stored playback credential, and what it says.
//!
//!   cargo run --example friends_probe

use librespot_core::{Session, SessionConfig, cache::Cache};

fn main() -> anyhow::Result<()> {
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

        match spotifast::friends::fetch(&session).await {
            Ok(friends) if friends.is_empty() => {
                println!("Spotify answered with no friend activity");
            }
            Ok(friends) => {
                println!("{} friends:", friends.len());
                for friend in friends {
                    println!(
                        "  {:<24} {} · {}  ({})",
                        friend.name,
                        friend.track.name,
                        friend.track.artist.name,
                        friend
                            .track
                            .context
                            .map(|context| context.name)
                            .unwrap_or_default(),
                    );
                }
            }
            Err(error) => println!("refused: {error:#}"),
        }
        anyhow::Ok(())
    })?;
    Ok(())
}
