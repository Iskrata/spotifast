//! Playlist search over the streaming session.
//!
//! Spotify's own clients search through its GraphQL service, `pathfinder`,
//! with the session's authorization. Its playlist results include the
//! playlists Spotify makes, the account's own Discover Weekly and Daily
//! Mixes among them, and nothing about it counts against a Web API app's
//! quota. The service is undocumented and names its query by a hash
//! Spotify may retire; any answer this module does not recognise is a
//! retry, so the Web API answers instead.

use anyhow::{Context as _, anyhow};
use librespot_core::Session;
use serde::Deserialize;

use crate::api::models::{Image, Owner, Page, Playlist};
use crate::session_reads::Failure;

const PATHFINDER: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
/// The persisted `searchPlaylists` query Spotify's clients send.
const SEARCH_PLAYLISTS: &str = "fc3a690182167dbad20ac7a03f842b97be4e9737710600874cb903f30112ad58";
/// As many as the shared app's search answers with.
pub const LIMIT: u32 = 20;

/// The first page of playlists matching `query`, in Spotify's order.
pub async fn playlists(session: &Session, query: &str) -> Result<Page<Playlist>, Failure> {
    let token = session.login5().auth_token().await?;
    let body = request_body(query);
    let mut request = http::Request::post(PATHFINDER)
        .header(
            http::header::AUTHORIZATION,
            format!("{} {}", token.token_type, token.access_token),
        )
        .header(http::header::ACCEPT, "application/json")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(body.into_bytes().into())
        .map_err(|error| Failure::Retry(error.into()))?;
    // Spotify's clients send it; the service answers without it too.
    if let Ok(client_token) = session.spclient().client_token().await
        && let Ok(value) = http::HeaderValue::from_str(&client_token)
    {
        request.headers_mut().insert("client-token", value);
    }
    let bytes = session.http_client().request_body(request).await?;
    parse(&bytes).map_err(Failure::Retry)
}

fn request_body(query: &str) -> String {
    serde_json::json!({
        "variables": {"searchTerm": query, "offset": 0, "limit": LIMIT},
        "operationName": "searchPlaylists",
        "extensions": {"persistedQuery": {"version": 1, "sha256Hash": SEARCH_PLAYLISTS}},
    })
    .to_string()
}

pub fn parse(bytes: &[u8]) -> anyhow::Result<Page<Playlist>> {
    let answer: Answer = serde_json::from_slice(bytes).context("unreadable search answer")?;
    let Some(found) = answer
        .data
        .and_then(|data| data.search)
        .and_then(|search| search.playlists)
    else {
        let reason = answer
            .errors
            .into_iter()
            .filter_map(|error| error.message)
            .next()
            .unwrap_or_else(|| "no playlists in the answer".into());
        return Err(anyhow!("search refused: {reason}"));
    };
    let items: Vec<Playlist> = found
        .items
        .into_iter()
        .filter_map(|wrapper| wrapper.data)
        .filter_map(WirePlaylist::into_playlist)
        .collect();
    let count = u32::try_from(items.len()).unwrap_or(u32::MAX);
    Ok(Page {
        total: found.total_count.unwrap_or(count).max(count),
        limit: LIMIT,
        offset: 0,
        next: None,
        items,
    })
}

#[derive(Deserialize)]
struct Answer {
    data: Option<Data>,
    #[serde(default)]
    errors: Vec<WireError>,
}

#[derive(Deserialize)]
struct WireError {
    message: Option<String>,
}

#[derive(Deserialize)]
struct Data {
    #[serde(rename = "searchV2")]
    search: Option<Search>,
}

#[derive(Deserialize)]
struct Search {
    playlists: Option<Found>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Found {
    #[serde(default)]
    items: Vec<Wrapper>,
    total_count: Option<u32>,
}

#[derive(Deserialize)]
struct Wrapper {
    data: Option<WirePlaylist>,
}

/// A result, or an error in its place, which has no `uri` and is dropped.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WirePlaylist {
    uri: Option<String>,
    name: Option<String>,
    description: Option<String>,
    images: Option<WireImages>,
    owner_v2: Option<WireOwnerWrapper>,
}

#[derive(Deserialize)]
struct WireImages {
    #[serde(default)]
    items: Vec<WireImage>,
}

#[derive(Deserialize)]
struct WireImage {
    #[serde(default)]
    sources: Vec<Image>,
}

#[derive(Deserialize)]
struct WireOwnerWrapper {
    data: Option<WireOwner>,
}

#[derive(Deserialize)]
struct WireOwner {
    uri: Option<String>,
    username: Option<String>,
    name: Option<String>,
}

impl WirePlaylist {
    fn into_playlist(self) -> Option<Playlist> {
        let uri = self.uri?;
        let id = uri.strip_prefix("spotify:playlist:")?.to_string();
        let owner = self.owner_v2.and_then(|owner| owner.data);
        let owner = owner.map_or_else(Owner::default, |owner| Owner {
            id: owner.username.or_else(|| {
                owner
                    .uri
                    .as_deref()
                    .and_then(|uri| uri.strip_prefix("spotify:user:"))
                    .map(str::to_string)
            }),
            display_name: owner.name.filter(|name| !name.is_empty()),
            uri: owner.uri,
        });
        let images = self
            .images
            .and_then(|images| images.items.into_iter().next())
            .map(|image| image.sources)
            .unwrap_or_default();
        Some(Playlist {
            id,
            name: self.name.unwrap_or_default(),
            uri,
            description: self.description.filter(|text| !text.is_empty()),
            images,
            owner,
            ..Playlist::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"{"data":{"searchV2":{"playlists":{
        "items":[
          {"__typename":"PlaylistResponseWrapper","data":{"__typename":"Playlist",
            "uri":"spotify:playlist:37i9dQZEVXcNZCpdy0yioi","name":"Discover Weekly",
            "description":"Your shortcut to hidden gems.","format":"discover-weekly",
            "images":{"items":[{"sources":[{"height":null,"url":"https://pickasso.spotifycdn.com/dw","width":null}]}]},
            "ownerV2":{"data":{"__typename":"User","name":"Spotify","uri":"spotify:user:spotify","username":"spotify"}}}},
          {"__typename":"PlaylistResponseWrapper","data":{"__typename":"GenericError"}},
          {"__typename":"PlaylistResponseWrapper","data":{"__typename":"Playlist",
            "uri":"spotify:playlist:4k7AJ58rAxkxxdCuJ2jZOV","name":"Mine","description":"",
            "images":{"items":[]},
            "ownerV2":{"data":{"__typename":"User","name":"Aj","uri":"spotify:user:aj"}}}}
        ],
        "pagingInfo":{"limit":20,"nextOffset":20},"totalCount":199}}}}"#;

    /// Results read as the Web API's playlists: Spotify's own included,
    /// with owner, cover and description, and an error in a result's place
    /// left out.
    #[test]
    fn results_read_as_web_api_playlists() {
        let page = parse(ANSWER.as_bytes()).unwrap();
        assert_eq!(page.total, 199);
        assert_eq!(page.next_offset(), None, "only the first page is asked");
        let ids: Vec<&str> = page.items.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["37i9dQZEVXcNZCpdy0yioi", "4k7AJ58rAxkxxdCuJ2jZOV"]);
        let weekly = &page.items[0];
        assert_eq!(weekly.uri, "spotify:playlist:37i9dQZEVXcNZCpdy0yioi");
        assert_eq!(weekly.owner.id.as_deref(), Some("spotify"));
        assert_eq!(weekly.owner_name(), "Spotify");
        assert_eq!(weekly.images[0].url, "https://pickasso.spotifycdn.com/dw");
        assert_eq!(weekly.images[0].width, None);
        assert_eq!(
            weekly.description.as_deref(),
            Some("Your shortcut to hidden gems.")
        );
        let mine = &page.items[1];
        assert_eq!(mine.owner.id.as_deref(), Some("aj"), "taken from the URI");
        assert_eq!(mine.owner_name(), "Aj");
        assert!(mine.images.is_empty());
        assert_eq!(mine.description, None);
        assert_eq!(mine.public, None, "the search does not say");
    }

    /// A retired query hash, or any answer without results, is not an
    /// empty search: the Web API is asked instead.
    #[test]
    fn an_answer_without_results_is_no_answer() {
        let retired = br#"{"errors":[{"message":"PersistedQueryNotFound"}]}"#;
        let error = parse(retired).unwrap_err().to_string();
        assert!(error.contains("PersistedQueryNotFound"), "{error}");
        assert!(parse(br#"{"data":{"searchV2":null}}"#).is_err());
        assert!(parse(b"<html>").is_err());
        let empty = parse(br#"{"data":{"searchV2":{"playlists":{"items":[]}}}}"#).unwrap();
        assert!(
            empty.items.is_empty(),
            "a search with no match is an answer"
        );
    }

    #[test]
    fn the_request_names_the_query_and_its_hash() {
        let body: serde_json::Value = serde_json::from_str(&request_body("daily mix")).unwrap();
        assert_eq!(body["operationName"], "searchPlaylists");
        assert_eq!(body["variables"]["searchTerm"], "daily mix");
        assert_eq!(body["variables"]["limit"], LIMIT);
        assert_eq!(
            body["extensions"]["persistedQuery"]["sha256Hash"],
            SEARCH_PLAYLISTS
        );
    }
}
