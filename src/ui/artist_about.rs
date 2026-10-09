//! The About the artist card of the now playing panel: the playing song's
//! first artist with a portrait, followers and genres when Spotify gives
//! them, Follow, and the start of the biography. The card opens the
//! artist's page.

use egui::{CornerRadius, Frame, Margin, Sense, Stroke, UiBuilder, Vec2, vec2};

use super::tokens;
use crate::api::models::pick_image;
use crate::app::{App, NowPlaying};
use crate::i18n::{gettext, pgettext};
use crate::model::{Action, Page};
use crate::theme::{self, Icon};

use super::buttons;
use super::widgets;

const PORTRAIT: f32 = 56.0;
/// How many rows of the biography show until it is expanded.
const BIOGRAPHY_ROWS: usize = 3;
/// How many genres the card names.
const GENRES: usize = 2;

/// What the card shows of one artist.
struct Shown {
    id: String,
    uri: String,
    name: String,
    portrait: Option<String>,
    followers: Option<u64>,
    genres: Vec<String>,
    biography: Option<String>,
}

impl Shown {
    /// What is known of the playing song's first artist, or `None` while
    /// nothing beyond its name is: the card stays away rather than show an
    /// empty box.
    fn of(app: &App, now: &NowPlaying) -> Option<Self> {
        if now.is_episode {
            return None;
        }
        let primary = now.artists.first()?;
        let id = primary.id.clone()?;
        let about = app.artist_about.get(&id)?;
        let artist = about.artist.get();
        let profile = about.profile.get();
        let portrait = artist
            .and_then(|artist| pick_image(&artist.images, (PORTRAIT * 2.0) as u32))
            .or_else(|| {
                profile.and_then(|profile| pick_image(&profile.portraits, (PORTRAIT * 2.0) as u32))
            })
            .map(str::to_string);
        let followers = artist
            .and_then(|artist| artist.followers.as_ref())
            .map(|followers| followers.total);
        let genres: Vec<String> = artist
            .map(|artist| artist.genres.iter().take(GENRES).cloned().collect())
            .unwrap_or_default();
        let biography = profile.and_then(|profile| profile.biography.clone());
        if portrait.is_none() && followers.is_none() && genres.is_empty() && biography.is_none() {
            return None;
        }
        Some(Self {
            uri: format!("spotify:artist:{id}"),
            name: artist
                .map(|artist| artist.name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| primary.name.clone()),
            id,
            portrait,
            followers,
            genres,
            biography,
        })
    }
}

/// Asks for the playing artist's details the first time it plays while the
/// panel shows, and draws the card once something is known. Returns
/// whether it was drawn.
pub(super) fn card(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying) -> bool {
    if let Some(id) = now.artists.first().and_then(|artist| artist.id.as_deref())
        && !now.is_episode
        && app.artist_about_due(id)
    {
        app.actions.push(Action::LoadArtistAbout(id.to_string()));
    }
    let Some(shown) = Shown::of(app, now) else {
        return false;
    };
    let palette = app.palette;
    let menu_id = ui.make_persistent_id(("artist-about-menu", &shown.uri));
    // Sensed before its contents, so Follow and See more keep their own
    // clicks and the rest of the card opens the artist.
    let scope = ui.scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
        Frame::new()
            .fill(palette.surface)
            .corner_radius(CornerRadius::same(tokens::radius::PANEL))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                buttons::on_card(ui, &palette);
                contents(app, ui, &shown);
            });
    });
    let response = scope
        .response
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &shown.name)
    });
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            CornerRadius::same(tokens::radius::PANEL),
            Stroke::new(1.0, palette.text.gamma_multiply(0.35)),
            egui::StrokeKind::Inside,
        );
    }
    if response.clicked() {
        app.actions
            .push(Action::Open(Page::Artist(shown.id.clone())));
    }
    egui::Popup::context_menu(&response)
        .id(menu_id)
        .frame(widgets::menu_frame(ui.ctx(), &palette))
        .show(|ui| widgets::context_menu_items(ui, app, &shown.uri, &shown.name, None));
    true
}

fn contents(app: &mut App, ui: &mut egui::Ui, shown: &Shown) {
    let palette = app.palette;
    let locale = app.locale;
    theme::text(
        ui,
        // Translators: The heading of the now playing panel's card about the playing song's artist.
        gettext(locale, "About the artist"),
        theme::semibold(13.0),
        palette.text,
    );
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 0.0);
        let (portrait, _) = ui.allocate_exact_size(Vec2::splat(PORTRAIT), Sense::hover());
        widgets::paint_cover(
            ui,
            &palette,
            shown.portrait.as_deref(),
            portrait,
            PORTRAIT / 2.0,
            Icon::User,
            Some(app.backend.art()),
        );
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            let width = ui.available_width();
            let name =
                widgets::ellipsized(ui, &shown.name, theme::bold(15.0), palette.text, width, 1);
            ui.add(egui::Label::new(name).selectable(false));
            if let Some(total) = shown.followers {
                let followers = super::artist::followers_label(locale, total);
                let followers = widgets::ellipsized(
                    ui,
                    &followers,
                    theme::regular(12.5),
                    palette.secondary,
                    width,
                    1,
                );
                ui.add(egui::Label::new(followers).selectable(false));
            }
            if !shown.genres.is_empty() {
                let genres = widgets::ellipsized(
                    ui,
                    &shown.genres.join(", "),
                    theme::regular(12.5),
                    palette.dim,
                    width,
                    1,
                );
                ui.add(egui::Label::new(genres).selectable(false));
            }
            // Only once Spotify has said whether the account follows them.
            if let Some(following) = app.is_saved(&shown.uri) {
                ui.add_space(6.0);
                let label = if following {
                    pgettext(locale, "artist", "Following")
                } else {
                    pgettext(locale, "artist", "Follow")
                };
                if buttons::secondary(ui, &palette, None, &label).clicked() {
                    app.actions.push(Action::ToggleSaved(shown.uri.clone()));
                }
            }
        });
    });
    if let Some(biography) = &shown.biography {
        ui.add_space(10.0);
        biography_text(app, ui, &shown.id, biography);
    }
}

/// The biography's first rows, with See more to read the rest in place.
fn biography_text(app: &App, ui: &mut egui::Ui, id: &str, biography: &str) {
    let palette = app.palette;
    let locale = app.locale;
    let expanded_id = ui.make_persistent_id(("artist-about-expanded", id));
    let expanded = ui.data(|data| data.get_temp::<bool>(expanded_id).unwrap_or(false));
    let width = ui.available_width();
    let font = theme::regular(13.0);
    let color = palette.text.gamma_multiply(0.8);
    let whole = widgets::ellipsized(ui, biography, font.clone(), color, width, usize::MAX);
    let longer = whole.rows.len() > BIOGRAPHY_ROWS;
    let galley = if expanded || !longer {
        whole
    } else {
        widgets::ellipsized(ui, biography, font, color, width, BIOGRAPHY_ROWS)
    };
    ui.add(egui::Label::new(galley).selectable(false));
    if longer {
        ui.add_space(4.0);
        let label = if expanded {
            gettext(locale, "Show less")
        } else {
            gettext(locale, "See more")
        };
        if theme::link(ui, label, theme::semibold(12.5), palette.text).clicked() {
            ui.data_mut(|data| data.insert_temp(expanded_id, !expanded));
        }
    }
}
