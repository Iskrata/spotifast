//! Which card's cover the moving album art background shows while the
//! pointer rests on a card.
//!
//! Cards note their art as they draw; the page reads the result once every
//! card has drawn. Leaving a card holds its colours for a moment, so sliding
//! across the gap between neighbouring cards does not dip back to the page's
//! colours, and a card whose colours are still being read keeps the previous
//! card's until they arrive.

use std::time::Duration;

use egui::{Context, Id};

/// How long a card's colours stay after the pointer leaves it, in seconds.
const HOLD: f64 = 0.3;

/// The card art under the pointer during this frame.
#[derive(Clone, Default)]
struct Hovered(Option<String>);

/// The card art the background shows, and when the pointer was last on it.
#[derive(Clone, PartialEq, Debug)]
struct Held {
    url: String,
    at: f64,
}

fn hovered_id() -> Id {
    Id::new("card-hover-art")
}

fn held_id() -> Id {
    Id::new("card-hover-held")
}

/// Forgets the previous frame's card, before the page draws.
pub fn begin_frame(ctx: &Context) {
    ctx.data_mut(|data| data.insert_temp(hovered_id(), Hovered(None)));
}

/// Notes that the pointer rests on a card showing `url`.
pub fn hover(ctx: &Context, url: &str) {
    ctx.data_mut(|data| data.insert_temp(hovered_id(), Hovered(Some(url.to_owned()))));
}

/// The art of the card under the pointer in the last frame drawn.
#[cfg(test)]
pub fn under_pointer(ctx: &Context) -> Option<String> {
    ctx.data(|data| data.get_temp::<Hovered>(hovered_id()))?.0
}

/// The card art the background shows once the page has drawn, if any.
/// `ready` says whether an art's colours are known, asking for them when
/// they are not.
pub fn art(ctx: &Context, ready: impl FnOnce(&str) -> bool) -> Option<String> {
    let now = ctx.input(|input| input.time);
    let (hovered, held) = ctx.data(|data| {
        (
            data.get_temp::<Hovered>(hovered_id()).unwrap_or_default().0,
            data.get_temp::<Held>(held_id()),
        )
    });
    let on_card = hovered.is_some();
    let held = hold(
        hovered.map(|url| {
            let ready = ready(&url);
            (url, ready)
        }),
        held,
        now,
    );
    ctx.data_mut(|data| match &held {
        Some(held) => {
            data.insert_temp(held_id(), held.clone());
        }
        None => data.remove::<Held>(held_id()),
    });
    let held = held?;
    if !on_card {
        // Let go once the hold runs out, even if the pointer stops moving.
        let left = (HOLD - (now - held.at)).max(0.0);
        ctx.request_repaint_after(Duration::from_secs_f64(left));
    }
    Some(held.url)
}

/// Which card art to keep showing. A card whose colours are known takes
/// over at once; one whose colours are still being read keeps the previous
/// card's for as long as the pointer stays on it; leaving every card keeps
/// the last one for [`HOLD`].
fn hold(hovered: Option<(String, bool)>, held: Option<Held>, now: f64) -> Option<Held> {
    match hovered {
        Some((url, true)) => Some(Held { url, at: now }),
        Some((_, false)) => held.map(|held| Held { at: now, ..held }),
        None => held.filter(|held| now - held.at < HOLD),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(url: &str, at: f64) -> Option<Held> {
        Some(Held {
            url: url.to_owned(),
            at,
        })
    }

    #[test]
    fn a_card_with_known_colours_takes_over_at_once() {
        assert_eq!(
            hold(Some(("b".to_owned(), true)), held("a", 1.0), 1.1),
            held("b", 1.1)
        );
        assert_eq!(
            hold(Some(("b".to_owned(), true)), None, 1.1),
            held("b", 1.1)
        );
    }

    #[test]
    fn a_card_still_being_read_keeps_the_previous_cards_colours() {
        assert_eq!(
            hold(Some(("b".to_owned(), false)), held("a", 1.0), 5.0),
            held("a", 5.0),
            "held for as long as the pointer stays"
        );
        assert_eq!(hold(Some(("b".to_owned(), false)), None, 5.0), None);
    }

    #[test]
    fn leaving_a_card_holds_its_colours_briefly() {
        assert_eq!(hold(None, held("a", 1.0), 1.1), held("a", 1.0));
        assert_eq!(hold(None, held("a", 1.0), 1.0 + HOLD), None);
        assert_eq!(hold(None, None, 1.0), None);
    }

    #[test]
    fn each_frame_starts_without_a_hovered_card() {
        let ctx = Context::default();
        hover(&ctx, "a");
        assert_eq!(art(&ctx, |_| true).as_deref(), Some("a"));
        begin_frame(&ctx);
        let mut asked = false;
        let shown = art(&ctx, |_| {
            asked = true;
            true
        });
        assert_eq!(shown.as_deref(), Some("a"), "the hold still shows it");
        assert!(!asked, "no card is under the pointer");
    }
}
