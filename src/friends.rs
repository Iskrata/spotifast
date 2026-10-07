//! What the account's friends are listening to.
//!
//! Spotify has no public API for this. Its desktop client reads the buddy
//! list from the same internal service the streaming session already talks
//! to, so Spotifast asks there with that session's own authorization. The
//! answer is unofficial and may change or stop without notice; anything it
//! does not recognise is left out rather than shown wrong.

use librespot_core::Session;
use serde::Deserialize;

const BUDDY_LIST: &str = "/presence-view/v1/buddylist";

/// One friend and the last song Spotify saw them play.
#[derive(Clone, Debug, PartialEq)]
pub struct Friend {
    pub uri: String,
    pub name: String,
    pub image_url: Option<String>,
    /// When the song started, in milliseconds since the Unix epoch.
    pub timestamp_ms: i64,
    pub track: FriendTrack,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FriendTrack {
    pub uri: String,
    pub name: String,
    pub image_url: Option<String>,
    pub artist: Link,
    pub album: Option<Link>,
    /// The playlist, album, or artist the song was playing from.
    pub context: Option<Link>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub uri: Option<String>,
    pub name: String,
}

/// Why the buddy list could not be read.
#[derive(Clone, Debug, PartialEq)]
pub enum Unavailable {
    /// Only the local playback session can ask, and it is not signed in.
    NoSession,
    Failed(String),
}

/// Friends who have played something, the most recent first.
pub async fn fetch(session: &Session) -> anyhow::Result<Vec<Friend>> {
    let bytes = session
        .spclient()
        .request_as_json(&http::Method::GET, BUDDY_LIST, None, None)
        .await?;
    parse(&bytes)
}

pub fn parse(bytes: &[u8]) -> anyhow::Result<Vec<Friend>> {
    let list: BuddyList = serde_json::from_slice(bytes)?;
    let mut friends: Vec<Friend> = list
        .friends
        .into_iter()
        .filter_map(Friend::from_wire)
        .collect();
    friends.sort_by_key(|friend| std::cmp::Reverse(friend.timestamp_ms));
    Ok(friends)
}

impl Friend {
    fn from_wire(wire: WireFriend) -> Option<Self> {
        let user = wire.user?;
        let track = wire.track?;
        let name = user.name.filter(|name| !name.is_empty())?;
        let track_name = track.name.filter(|name| !name.is_empty())?;
        Some(Self {
            uri: user.uri?,
            name,
            image_url: user.image_url.filter(|url| !url.is_empty()),
            timestamp_ms: wire.timestamp.unwrap_or_default(),
            track: FriendTrack {
                uri: track.uri?,
                name: track_name,
                image_url: track.image_url.filter(|url| !url.is_empty()),
                artist: track.artist.and_then(WireLink::into_link)?,
                album: track.album.and_then(WireLink::into_link),
                context: track.context.and_then(WireLink::into_link),
            },
        })
    }

    /// The friend's Spotify user id, from their URI.
    pub fn user_id(&self) -> &str {
        self.uri.rsplit(':').next().unwrap_or(&self.uri)
    }
}

#[derive(Deserialize)]
struct BuddyList {
    #[serde(default)]
    friends: Vec<WireFriend>,
}

#[derive(Deserialize)]
struct WireFriend {
    timestamp: Option<i64>,
    user: Option<WireUser>,
    track: Option<WireTrack>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireUser {
    uri: Option<String>,
    name: Option<String>,
    image_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireTrack {
    uri: Option<String>,
    name: Option<String>,
    image_url: Option<String>,
    artist: Option<WireLink>,
    album: Option<WireLink>,
    context: Option<WireLink>,
}

#[derive(Deserialize)]
struct WireLink {
    uri: Option<String>,
    name: Option<String>,
}

impl WireLink {
    fn into_link(self) -> Option<Link> {
        let name = self.name.filter(|name| !name.is_empty())?;
        Some(Link {
            uri: self.uri.filter(|uri| !uri.is_empty()),
            name,
        })
    }
}

/// How long ago a friend's song started, in a few characters: "now" while
/// it may still be playing, then minutes, hours, days, and weeks.
pub fn age_label(now_ms: i64, timestamp_ms: i64) -> Age {
    let minutes = (now_ms - timestamp_ms).max(0) / 60_000;
    match minutes {
        0..=9 => Age::Now,
        10..=59 => Age::Minutes(minutes),
        60..=1439 => Age::Hours(minutes / 60),
        1440..=10079 => Age::Days(minutes / 1440),
        _ => Age::Weeks(minutes / 10080),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Age {
    Now,
    Minutes(i64),
    Hours(i64),
    Days(i64),
    Weeks(i64),
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"{"friends":[
        {"timestamp":1700000000000,
         "user":{"uri":"spotify:user:ann","name":"Ann","imageUrl":"https://i.scdn.co/image/ann"},
         "track":{"uri":"spotify:track:t1","name":"Rosewood","imageUrl":"https://i.scdn.co/image/t1",
                  "album":{"uri":"spotify:album:a1","name":"Fragments"},
                  "artist":{"uri":"spotify:artist:r1","name":"Bonobo"},
                  "context":{"uri":"spotify:playlist:p1","name":"Late night focus","index":0}}},
        {"timestamp":1700000600000,
         "user":{"uri":"spotify:user:ben","name":"Ben"},
         "track":{"uri":"spotify:track:t2","name":"Tides",
                  "artist":{"uri":"spotify:artist:r2","name":"Little Simz"},
                  "context":{"uri":"","name":""},
                  "somethingNew":true}},
        {"timestamp":1700000700000,
         "user":{"uri":"spotify:user:cat","name":"Cat"}}
    ]}"#;

    #[test]
    fn friends_come_newest_first_with_what_they_played() {
        let friends = parse(ANSWER.as_bytes()).unwrap();
        assert_eq!(friends.len(), 2, "a friend with no song is left out");
        assert_eq!(friends[0].name, "Ben");
        assert_eq!(friends[0].track.artist.name, "Little Simz");
        assert_eq!(
            friends[0].track.context, None,
            "an empty context is no context"
        );
        assert_eq!(friends[0].image_url, None);
        let ann = &friends[1];
        assert_eq!(ann.user_id(), "ann");
        assert_eq!(ann.track.name, "Rosewood");
        assert_eq!(
            ann.track.album,
            Some(Link {
                uri: Some("spotify:album:a1".into()),
                name: "Fragments".into()
            })
        );
        assert_eq!(ann.track.context.as_ref().unwrap().name, "Late night focus");
    }

    #[test]
    fn an_empty_or_unfamiliar_answer_is_no_friends() {
        assert!(parse(b"{}").unwrap().is_empty());
        assert!(parse(br#"{"friends":[{"user":null}]}"#).unwrap().is_empty());
        assert!(parse(b"not json").is_err());
    }

    #[test]
    fn ages_round_down_into_the_largest_unit() {
        let minute = 60_000;
        assert_eq!(age_label(0, 0), Age::Now);
        assert_eq!(age_label(5 * minute, 0), Age::Now);
        assert_eq!(age_label(0, 5 * minute), Age::Now, "a clock behind is now");
        assert_eq!(age_label(12 * minute, 0), Age::Minutes(12));
        assert_eq!(age_label(150 * minute, 0), Age::Hours(2));
        assert_eq!(age_label(3 * 1440 * minute, 0), Age::Days(3));
        assert_eq!(age_label(15 * 1440 * minute, 0), Age::Weeks(2));
    }
}
