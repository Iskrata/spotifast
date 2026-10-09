//! The playing song at the top of the Friend Activity panel: a small cover
//! with its title and artists, Like and Add to playlist, the lyrics as they
//! are sung, about the artist, and what plays next.

use egui::{CornerRadius, Frame, Margin, Sense, Stroke, Vec2, vec2};

use super::tokens;
use crate::app::{App, NowPlaying};
use crate::i18n::gettext;
use crate::lyrics::Lyrics;
use crate::model::{Action, Loadable, Page};
use crate::theme::{self, Icon};

use super::{player_bar, widgets};

const COVER: f32 = 64.0;
/// How many lyric lines the card shows: the one being sung and those after.
const LYRIC_LINES: usize = 3;
/// How many songs of the queue the panel shows.
const QUEUE_ROWS: usize = 2;
const SECTION_GAP: f32 = 16.0;

/// Draws the section for `now`, ending with room before what follows.
pub(super) fn section(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying) {
    song(app, ui, now);
    ui.add_space(12.0);
    if lyrics_card(app, ui, now) {
        ui.add_space(SECTION_GAP);
    }
    if super::artist_about::card(app, ui, now) {
        ui.add_space(SECTION_GAP);
    }
    // The whole queue on screen already lists what plays next.
    let queue_shown = app.show_queue_panel || matches!(app.page(), Page::Queue);
    if !queue_shown && up_next(app, ui) {
        ui.add_space(SECTION_GAP);
    }
}

fn song(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying) {
    let palette = app.palette;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 0.0);
        let (cover, response) = ui.allocate_exact_size(Vec2::splat(COVER), Sense::click());
        widgets::paint_cover(
            ui,
            &palette,
            now.art_small.as_deref().or(now.art_url.as_deref()),
            cover,
            tokens::points(tokens::radius::ROW),
            Icon::Music,
            Some(app.backend.art()),
        );
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            player_bar::open_playing(app, now);
        }
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 1.0;
            if theme::link(ui, &now.title, theme::bold(15.0), palette.text).clicked() {
                player_bar::open_playing(app, now);
            }
            ui.horizontal_top(|ui| {
                player_bar::byline(ui, app, now, theme::regular(13.0), palette.secondary);
            });
            if let Some(item) = app.now_playing_item() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    widgets::heart_button(ui, app, &now.uri, 16.0);
                    widgets::add_to_playlist_button(ui, app, std::slice::from_ref(&item), 16.0);
                });
            }
        });
    });
}

/// The line being sung and the ones after it, on a card in the art's
/// colour that opens the lyrics panel. Returns whether it was drawn: there
/// is no card until the playing song's words are here.
fn lyrics_card(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying) -> bool {
    if app.lyrics_uri.as_deref() != Some(now.uri.as_str()) {
        return false;
    }
    let Loadable::Loaded(Some(lyrics)) = &app.lyrics else {
        return false;
    };
    if lyrics.instrumental || lyrics.lines.is_empty() {
        return false;
    }
    let lyrics = lyrics.clone();
    let palette = app.palette;
    let (start, active) = preview(&lyrics, now.position_ms, now.duration_ms);
    let fill = app.now_playing_tint().unwrap_or(palette.surface);
    let quiet = palette.text.gamma_multiply(0.6);
    let card = Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(tokens::radius::PANEL))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            theme::text(
                ui,
                gettext(app.locale, "Lyrics"),
                theme::semibold(13.0),
                palette.text,
            );
            ui.add_space(2.0);
            for (index, line) in lyrics
                .lines
                .iter()
                .enumerate()
                .skip(start)
                .take(LYRIC_LINES)
            {
                let sung = active == Some(index);
                let (font, color) = if sung {
                    (theme::bold(15.0), palette.text)
                } else {
                    (theme::regular(15.0), quiet)
                };
                // A timed line with no words is the band playing on.
                let text = if line.text.is_empty() {
                    "\u{266a}"
                } else {
                    line.text.as_str()
                };
                let galley = widgets::ellipsized(ui, text, font, color, ui.available_width(), 2);
                ui.add(egui::Label::new(galley).selectable(false));
            }
        });
    let rect = card.response.rect;
    let response = ui
        .interact(rect, ui.id().with("lyrics-preview"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            gettext(app.locale, "Show lyrics"),
        )
    });
    if response.hovered() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(tokens::radius::PANEL),
            Stroke::new(1.0, palette.text.gamma_multiply(0.35)),
            egui::StrokeKind::Inside,
        );
    }
    if response.clicked() {
        app.actions.push(Action::ToggleLyricsPanel);
    }
    super::lyrics::repaint_at_next_line(ui.ctx(), &lyrics, now);
    true
}

/// The first line the card shows and the line being sung, if any. Timed
/// lyrics start at the sung line, or the first before it begins. Words
/// without timing follow the clock, as the lyrics panel does, and light
/// no line.
fn preview(lyrics: &Lyrics, position_ms: u32, duration_ms: u32) -> (usize, Option<usize>) {
    let last = lyrics.lines.len().saturating_sub(1);
    if lyrics.synced {
        let active = lyrics.active_line(position_ms);
        return (active.unwrap_or(0).min(last), active);
    }
    if duration_ms == 0 {
        return (0, None);
    }
    let fraction = (f64::from(position_ms) / f64::from(duration_ms)).clamp(0.0, 1.0);
    (
        ((fraction * lyrics.lines.len() as f64) as usize).min(last),
        None,
    )
}

/// The next songs in the queue with a link to the whole of it. Returns
/// whether anything was drawn.
fn up_next(app: &mut App, ui: &mut egui::Ui) -> bool {
    let count = app
        .queue
        .get()
        .map_or(0, |queue| queue.queue.len().min(QUEUE_ROWS));
    if count == 0 {
        return false;
    }
    let palette = app.palette;
    egui::Sides::new().shrink_left().show(
        ui,
        |ui| {
            theme::text(
                ui,
                gettext(app.locale, "Next in queue"),
                theme::semibold(14.0),
                palette.text,
            );
        },
        |ui| {
            if theme::link(
                ui,
                gettext(app.locale, "Open queue"),
                theme::regular(12.5),
                palette.secondary,
            )
            .clicked()
            {
                app.actions.push(Action::ToggleQueuePanel);
            }
        },
    );
    ui.add_space(4.0);
    for index in 0..count {
        super::queue::queue_row(app, ui, index, true, 0.0);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::preview;
    use crate::lyrics::{Line, Lyrics};

    fn lyrics(synced: bool) -> Lyrics {
        Lyrics {
            lines: (0..10)
                .map(|index| Line {
                    at_ms: synced.then_some(index * 1_000 + 1_000),
                    text: format!("line {index}"),
                })
                .collect(),
            synced,
            instrumental: false,
        }
    }

    #[test]
    fn timed_lyrics_start_at_the_sung_line() {
        let lyrics = lyrics(true);
        assert_eq!(preview(&lyrics, 0, 20_000), (0, None), "before the words");
        assert_eq!(preview(&lyrics, 3_500, 20_000), (2, Some(2)));
        assert_eq!(preview(&lyrics, 60_000, 20_000), (9, Some(9)));
    }

    #[test]
    fn untimed_lyrics_follow_the_clock_without_lighting_a_line() {
        let lyrics = lyrics(false);
        assert_eq!(preview(&lyrics, 0, 20_000), (0, None));
        assert_eq!(preview(&lyrics, 10_000, 20_000), (5, None));
        assert_eq!(preview(&lyrics, 20_000, 20_000), (9, None));
        assert_eq!(preview(&lyrics, 5_000, 0), (0, None), "no length known");
    }
}
