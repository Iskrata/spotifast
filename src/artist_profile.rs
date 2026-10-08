//! What the playback session knows about an artist that the Web API does
//! not say: the biography Spotify's own clients show, and portraits.
//!
//! The session reads the artist's catalogue metadata, the same record
//! librespot reads for playback, so this costs no Web API quota.

use librespot_core::{Session, SpotifyUri, spotify_id::SpotifyId};
use librespot_metadata::{Artist as SessionArtist, Metadata as _};

use crate::api::models::Image;

/// The biography and portraits of one artist.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtistProfile {
    /// The biography as plain text, paragraphs apart by a blank line;
    /// `None` when Spotify has none.
    pub biography: Option<String>,
    pub portraits: Vec<Image>,
}

/// Reads the artist `id` (base62) over the session.
pub async fn read(session: &Session, id: &str) -> anyhow::Result<ArtistProfile> {
    let uri = SpotifyUri::Artist {
        id: SpotifyId::from_base62(id)?,
    };
    let artist = SessionArtist::get(session, &uri).await?;
    Ok(profile(&artist))
}

fn profile(artist: &SessionArtist) -> ArtistProfile {
    let biography = artist
        .biographies
        .iter()
        .map(|biography| plain_text(&biography.text))
        .find(|text| !text.is_empty());
    let mut portraits = crate::session_reads::images(&artist.portrait_group);
    if portraits.is_empty() {
        portraits = crate::session_reads::images(&artist.portraits);
    }
    ArtistProfile {
        biography,
        portraits,
    }
}

/// Spotify writes biographies as light HTML: links to other artists,
/// line and paragraph breaks, and escaped characters. Keeps the words and
/// the paragraphs and drops the markup.
pub fn plain_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(['<', '&']) {
        text.push_str(&rest[..start]);
        rest = &rest[start..];
        if rest.starts_with('<') {
            // A lone `<` is the text's own.
            let Some(end) = rest.find('>') else {
                break;
            };
            if breaks_line(&rest[1..end]) {
                text.push('\n');
            }
            rest = &rest[end + 1..];
        } else if let Some((character, length)) = entity(rest) {
            text.push(character);
            rest = &rest[length..];
        } else {
            text.push('&');
            rest = &rest[1..];
        }
    }
    text.push_str(rest);
    text.split('\n')
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Whether a tag, without its angle brackets, ends a line or paragraph.
fn breaks_line(tag: &str) -> bool {
    let name = tag
        .trim_start_matches('/')
        .split(|c: char| c.is_whitespace() || c == '/')
        .next()
        .unwrap_or_default();
    ["br", "p", "div"]
        .iter()
        .any(|known| name.eq_ignore_ascii_case(known))
}

/// The character an entity at the start of `text` stands for, and its
/// length.
fn entity(text: &str) -> Option<(char, usize)> {
    let end = text.get(..12).unwrap_or(text).find(';')?;
    let name = &text[1..end];
    let character = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((character, end + 1))
}

#[cfg(test)]
mod tests {
    use super::plain_text;

    #[test]
    fn links_lose_their_markup_and_keep_their_words() {
        assert_eq!(
            plain_text(
                "Formed with <a href=\"spotify:artist:4Z8W4fKeB5YxbusRsdQVPb\">Radiohead</a>'s drummer."
            ),
            "Formed with Radiohead's drummer."
        );
    }

    #[test]
    fn breaks_become_paragraphs_and_spaces_collapse() {
        assert_eq!(
            plain_text("  First   line.<br/>Second line.<br><br>\n<p>Third</p>  "),
            "First line.\n\nSecond line.\n\nThird"
        );
    }

    #[test]
    fn entities_are_read() {
        assert_eq!(
            plain_text("Simon &amp; Garfunkel &quot;live&quot; &#39;66 &#x2014; &nbsp;Paris"),
            "Simon & Garfunkel \"live\" '66 \u{2014} Paris"
        );
        assert_eq!(
            plain_text("R&B and &unknown; 3 < 4"),
            "R&B and &unknown; 3 < 4"
        );
    }

    #[test]
    fn nothing_but_markup_is_empty() {
        assert_eq!(plain_text("<p> </p><br/>"), "");
    }
}
