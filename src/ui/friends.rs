//! Friend Activity: what the people the account follows last played, under
//! the song playing here.

use std::time::Duration;

use egui::{Align, Frame, Layout, Margin, Sense, UiBuilder, vec2};

use crate::app::App;
use crate::friends::{Age, Friend};
use crate::i18n::{Locale, gettext, pgettext};
use crate::model::{Action, Dialog, Loadable, Page};
use crate::theme::{self, Icon};

use super::widgets;

const AVATAR: f32 = 40.0;

pub fn side_panel(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let fit = super::yielding_panel(
        ui.ctx(),
        "friends-panel",
        theme::SIDE_PANEL_MIN_WIDTH..=480.0,
        app.settings.friends_width,
        ui.available_width() - super::topbar::least_width(ui.ctx()),
    );
    let panel = egui::Panel::right("friends-panel")
        .resizable(true)
        .default_size(app.settings.friends_width)
        .size_range(fit.range.clone())
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(palette.panel)
                .inner_margin(Margin::symmetric(12, 12)),
        );
    let response = panel.show(ui, |ui| {
        let window_controls =
            super::window_controls_reservation(ui.ctx(), true, false, ui.available_width());
        ui.add_space(window_controls.queue_top);
        // While a song plays, it heads the panel and Friend Activity
        // follows as a section of its own, with its Refresh beside it.
        let now = app.now_playing();
        let title = if now.is_some() {
            gettext(locale, "Now playing")
        } else {
            gettext(locale, "Friend Activity")
        };
        let mut close = false;
        let mut refresh = false;
        egui::Sides::new().shrink_left().show(
            ui,
            |ui| {
                ui.add_space(4.0);
                theme::text(ui, title, theme::bold(16.0), palette.text);
            },
            |ui| {
                close = theme::icon_button(
                    ui,
                    Icon::X,
                    18.0,
                    palette.secondary,
                    palette.text,
                    &gettext(locale, "Close"),
                )
                .clicked();
                if now.is_none() {
                    refresh = refresh_button(ui, &palette, locale);
                }
            },
        );
        if close {
            app.actions.push(Action::ToggleFriendsPanel);
        }
        if refresh {
            app.actions.push(Action::RefreshFriends(true));
        } else if app.friends_stale(false) {
            app.actions.push(Action::RefreshFriends(false));
        }
        // Wake for the next refresh and to move the ages on.
        ui.ctx().request_repaint_after(Duration::from_secs(30));
        ui.add_space(10.0);
        egui::ScrollArea::vertical()
            .id_salt("friends-panel-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Clear of the scroll bar, which overlays the right edge.
                Frame::new()
                    .inner_margin(Margin {
                        right: 12,
                        ..Margin::ZERO
                    })
                    .show(ui, |ui| {
                        if let Some(now) = &now {
                            super::now_playing_panel::section(app, ui, now);
                            if friends_heading(ui, &palette, locale) {
                                app.actions.push(Action::RefreshFriends(true));
                            }
                            ui.add_space(8.0);
                        }
                        contents(app, ui);
                    });
            });
    });
    let width = response.response.rect.width();
    if (width - app.settings.friends_width).abs() > 1.0
        && super::panel_width_chosen(ui.ctx(), "friends-panel", &fit)
    {
        app.settings.friends_width = width;
        app.actions.push(Action::SettingsChanged);
    }
}

fn refresh_button(ui: &mut egui::Ui, palette: &theme::Palette, locale: Locale) -> bool {
    theme::icon_button(
        ui,
        Icon::Refresh,
        16.0,
        palette.secondary,
        palette.text,
        &gettext(locale, "Refresh"),
    )
    .clicked()
}

/// Friend Activity's heading below the playing song. Returns whether its
/// Refresh was clicked.
fn friends_heading(ui: &mut egui::Ui, palette: &theme::Palette, locale: Locale) -> bool {
    let mut refresh = false;
    egui::Sides::new().shrink_left().show(
        ui,
        |ui| {
            theme::text(
                ui,
                gettext(locale, "Friend Activity"),
                theme::semibold(14.0),
                palette.text,
            );
        },
        |ui| refresh = refresh_button(ui, palette, locale),
    );
    refresh
}

fn contents(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    match &app.friends {
        Loadable::Loaded(friends) if !friends.is_empty() => {
            let now = now_ms();
            let friends = friends.clone();
            for friend in &friends {
                row(app, ui, friend, now);
                ui.add_space(6.0);
            }
            ui.add_space(8.0);
            note(
                ui,
                &palette,
                &gettext(
                    locale,
                    "Spotify does not publish friend activity, so this list can stop working without notice.",
                ),
            );
        }
        Loadable::Loaded(_) => note(
            ui,
            &palette,
            &gettext(
                locale,
                "When the people you follow share their listening on Spotify, it shows here.",
            ),
        ),
        Loadable::Failed(_) if app.friends_need_session => {
            note(
                ui,
                &palette,
                &gettext(
                    locale,
                    "Friend activity reads through local playback. Turn on playback on this computer in Settings.",
                ),
            );
            ui.add_space(8.0);
            if theme::soft_button(
                ui,
                &palette,
                Some(Icon::Settings),
                &gettext(locale, "Open Settings"),
                false,
            )
            .clicked()
            {
                app.actions.push(Action::Open(Page::Settings));
            }
        }
        Loadable::Failed(error) => {
            log::debug!("friend activity unavailable: {error}");
            note(
                ui,
                &palette,
                &gettext(locale, "Spotify did not return friend activity."),
            );
            ui.add_space(8.0);
            if theme::soft_button(
                ui,
                &palette,
                Some(Icon::Refresh),
                &gettext(locale, "Retry"),
                false,
            )
            .clicked()
            {
                app.actions.push(Action::RefreshFriends(true));
            }
        }
        Loadable::NotLoaded | Loadable::Loading => {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| ui.spinner());
        }
    }
}

fn note(ui: &mut egui::Ui, palette: &theme::Palette, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(theme::regular(12.5))
                .color(palette.dim),
        )
        .wrap(),
    );
}

fn row(app: &mut App, ui: &mut egui::Ui, friend: &Friend, now: i64) {
    let palette = app.palette;
    let age = crate::friends::age_label(now, friend.timestamp_ms);
    let menu_id = ui.make_persistent_id(("friend-menu", &friend.uri));
    // The row answers a right-click, and its More button stands in for the
    // age while the pointer is over the row or its menu is open. Sensed
    // before its contents, so the links inside keep their own clicks. The
    // pointer counts while it rests on the button too: the row is not
    // `hovered` then, and the button would vanish under it and flicker.
    let scope = ui.scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
        let show_more =
            ui.response().contains_pointer() || egui::Popup::is_id_open(ui.ctx(), menu_id);
        row_contents(app, ui, friend, age, show_more.then_some(menu_id));
    });
    egui::Popup::context_menu(&scope.response)
        .frame(widgets::menu_frame(&palette))
        .show(|ui| friend_menu(ui, app, friend));
}

fn friend_menu(ui: &mut egui::Ui, app: &mut App, friend: &Friend) {
    ui.set_min_width(180.0);
    let palette = app.palette;
    // Translators: Stop following a person in Friend Activity.
    let unfollow = pgettext(app.locale, "user", "Unfollow");
    if widgets::menu_item(ui, &palette, Some(Icon::CircleX), &unfollow) {
        app.actions
            .push(Action::ShowDialog(Dialog::ConfirmUnfollowFriend {
                uri: friend.uri.clone(),
                name: friend.name.clone(),
            }));
    }
}

fn row_contents(
    app: &mut App,
    ui: &mut egui::Ui,
    friend: &Friend,
    age: Age,
    more: Option<egui::Id>,
) {
    let palette = app.palette;
    let locale = app.locale;
    let art = app.backend.art().clone();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 1.0);
        let (avatar, _) = ui.allocate_exact_size(vec2(AVATAR, AVATAR), Sense::hover());
        widgets::paint_cover(
            ui,
            &palette,
            friend.image_url.as_deref(),
            avatar,
            AVATAR / 2.0,
            Icon::User,
            Some(&art),
        );
        if age == Age::Now {
            // A small dot on the avatar while the song may still be playing.
            let center = avatar.right_bottom() - vec2(5.0, 5.0);
            ui.painter().circle_filled(center, 5.5, palette.panel);
            ui.painter().circle_filled(center, 4.0, palette.accent);
        }
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let label = age_text(locale, age);
                let label_width = ui
                    .painter()
                    .layout_no_wrap(label.clone(), theme::regular(12.0), palette.dim)
                    .size()
                    .x;
                ui.allocate_ui_with_layout(
                    vec2((ui.available_width() - label_width - 6.0).max(0.0), 18.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        theme::text(ui, &friend.name, theme::medium(14.0), palette.text);
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(menu_id) = more {
                        more_button(app, ui, friend, menu_id, label_width);
                    } else if age == Age::Now {
                        theme::text(ui, label, theme::medium(12.0), palette.accent);
                    } else {
                        theme::text(ui, label, theme::regular(12.0), palette.dim);
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let track = theme::link(
                    ui,
                    &friend.track.name,
                    theme::regular(12.5),
                    palette.secondary,
                );
                if track.clicked()
                    && let Some(uri) = friend
                        .track
                        .album
                        .as_ref()
                        .and_then(|album| album.uri.clone())
                {
                    app.actions.push(Action::OpenUri(uri));
                }
                theme::text(ui, " · ", theme::regular(12.5), palette.dim);
                let artist = theme::link(
                    ui,
                    &friend.track.artist.name,
                    theme::regular(12.5),
                    palette.secondary,
                );
                if artist.clicked()
                    && let Some(uri) = friend.track.artist.uri.clone()
                {
                    app.actions.push(Action::OpenUri(uri));
                }
            });
            if let Some(context) = &friend.track.context {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (icon, _) = ui.allocate_exact_size(vec2(12.0, 16.0), Sense::hover());
                    theme::paint_icon(
                        ui,
                        context_icon(context.uri.as_deref()),
                        icon,
                        12.0,
                        palette.dim,
                    );
                    let link = theme::link(ui, &context.name, theme::regular(12.0), palette.dim);
                    if link.clicked()
                        && let Some(uri) = context.uri.clone()
                    {
                        app.actions.push(Action::OpenUri(uri));
                    }
                });
            }
        });
    });
}

/// The row's More button, in the age's place and no taller than its line,
/// so showing it moves nothing.
fn more_button(app: &mut App, ui: &mut egui::Ui, friend: &Friend, menu_id: egui::Id, width: f32) {
    let palette = app.palette;
    let (slot, _) = ui.allocate_exact_size(vec2(width, 18.0), Sense::hover());
    let rect = egui::Rect::from_center_size(
        egui::pos2(slot.right() - 12.0, slot.center().y),
        vec2(24.0, 24.0),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
    );
    let button = theme::icon_button(
        &mut child,
        Icon::Ellipsis,
        12.0,
        palette.secondary,
        palette.text,
        // Translators: {name} is a person in Friend Activity.
        &gettext(app.locale, "More options for {name}").replace("{name}", &friend.name),
    );
    egui::Popup::menu(&button)
        .id(menu_id)
        .frame(widgets::menu_frame(&palette))
        .show(|ui| friend_menu(ui, app, friend));
}

fn context_icon(uri: Option<&str>) -> Icon {
    match uri.and_then(|uri| uri.split(':').nth(1)) {
        Some("album") => Icon::Disc,
        Some("artist") => Icon::User,
        _ => Icon::ListMusic,
    }
}

fn age_text(locale: Locale, age: Age) -> String {
    match age {
        // Translators: a friend's song may still be playing.
        Age::Now => gettext(locale, "Now").into_owned(),
        // Translators: {count} minutes ago, kept as short as possible.
        Age::Minutes(count) => {
            gettext(locale, "{count} min").replace("{count}", &count.to_string())
        }
        // Translators: {count} hours ago, kept as short as possible.
        Age::Hours(count) => gettext(locale, "{count} hr").replace("{count}", &count.to_string()),
        // Translators: {count} days ago, kept as short as possible.
        Age::Days(count) => gettext(locale, "{count} d").replace("{count}", &count.to_string()),
        // Translators: {count} weeks ago, kept as short as possible.
        Age::Weeks(count) => gettext(locale, "{count} wk").replace("{count}", &count.to_string()),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}
