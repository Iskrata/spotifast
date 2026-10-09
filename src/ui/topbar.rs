//! Navigation arrows, search, and the account menu above every page.

use std::sync::Arc;

use egui::{Align, Galley, Layout, Sense, Vec2, pos2, vec2};

use super::buttons::{AvatarButton, IconButton, IconSize};
use crate::api::models::pick_image;
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Page};
use crate::theme::{self, Icon, Palette};

/// The gap the bar keeps between everything it lays out.
const ITEM_SPACING: f32 = 8.0;
/// The navigation buttons and the account avatar.
const NAV_SIZE: IconSize = IconSize::Standard;
const AVATAR_SIZE: IconSize = IconSize::Large;
const SPINNER_SIZE: f32 = 15.0;
/// A badge is as tall as its text plus this, and as wide as its text plus
/// the padding its own label needs.
const BADGE_PADDING_Y: f32 = 12.0;
const DEVICE_BADGE_PADDING: f32 = 30.0;
/// The width the search field aims for, the most it ever takes, and the
/// least it shrinks to before the badge gives up its label instead.
const SEARCH_IDEAL: f32 = 200.0;
const SEARCH_MAX: f32 = 440.0;
const SEARCH_FLOOR: f32 = 130.0;
// After the badge collapses, a right panel can leave less than 130 points.
// Keep the original 80-point minimum inside the page's own toolbar.
const SEARCH_MIN: f32 = 80.0;
/// Everything at the right end whose width never changes: the page padding
/// and the avatar. Settings, the Winamp mini player and MilkDrop live in
/// the account menu. The spinner and the badge are measured on top of it
/// because they come and go.
const RIGHT_CONTROLS_WIDTH: f32 = super::widgets::PAGE_PADDING + AVATAR_SIZE.hit();

/// What precedes the field until the bar has drawn once: the page padding,
/// the back and forward buttons and the gaps after them.
const LEAD_GUESS: f32 =
    super::widgets::PAGE_PADDING + 2.0 * NAV_SIZE.hit() + 3.0 * ITEM_SPACING + 8.0;
/// A badge collapsed to its icon: a square as tall as its 12.5 pt label.
const BADGE_CHIP: f32 = 15.0 + BADGE_PADDING_Y;

fn lead_id() -> egui::Id {
    egui::Id::new("topbar-lead")
}

/// The narrowest the bar, and so the page under it, can be before its
/// controls run into each other: the narrowest field, with the spinner and
/// the device badge as an icon. Counting them even while they are away
/// keeps the panels and the window from changing width as they come and go.
pub fn least_width(ctx: &egui::Context) -> f32 {
    let lead = ctx
        .data(|data| data.get_temp(lead_id()))
        .unwrap_or(LEAD_GUESS);
    least_width_after(lead)
}

fn least_width_after(lead: f32) -> f32 {
    lead + SEARCH_MIN
        + RIGHT_CONTROLS_WIDTH
        + SPINNER_SIZE
        + ITEM_SPACING
        + ITEM_SPACING
        + BADGE_CHIP
}

/// How the top bar divides itself for one window width.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TopbarFit {
    /// How wide the search field may be.
    search: f32,
    /// Whether the badge has the room to spell itself out.
    labels: bool,
}

/// Divide the bar. The search field keeps the half it has always had, but
/// never so much that the right end has to reach over it, and the badge
/// falls back to its icon before the field shrinks past reading size.
///
/// `labelled` and `icons` are what the badge asks for with and without its
/// text, already including the spacing that precedes it.
fn topbar_fit(room: f32, controls: f32, labelled: f32, icons: f32) -> TopbarFit {
    // SEARCH_IDEAL is above SEARCH_FLOOR, so the clamp below is well ordered.
    let ideal = (room * 0.5).clamp(SEARCH_IDEAL, SEARCH_MAX);
    let labels = room - controls - labelled >= SEARCH_FLOOR;
    let badges = if labels { labelled } else { icons };
    TopbarFit {
        search: (room - controls - badges).clamp(SEARCH_MIN, ideal),
        labels,
    }
}

/// What a badge asks of the bar, including the spacing before it.
fn badge_width(galley: Option<&Arc<Galley>>, padding: f32, labels: bool) -> f32 {
    galley.map_or(0.0, |galley| {
        ITEM_SPACING
            + if labels {
                galley.size().x + padding
            } else {
                galley.size().y + BADGE_PADDING_Y
            }
    })
}

/// A pill at the right end of the bar: an icon with its label, or the icon
/// alone once the bar is too narrow to spare the room for words.
fn badge(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    galley: Arc<Galley>,
    padding: f32,
    labels: bool,
) -> egui::Response {
    let height = galley.size().y + BADGE_PADDING_Y;
    let size = if labels {
        vec2(galley.size().x + padding, height)
    } else {
        Vec2::splat(height)
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    // Collapsed to its icon the badge has no text on screen, so its label
    // reaches a screen reader as the widget's name.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), galley.text())
    });
    ui.painter().rect_filled(
        rect,
        super::tokens::capsule(rect.height()),
        palette.accent.gamma_multiply(0.16),
    );
    let icon_center = if labels {
        pos2(rect.left() + 14.0, rect.center().y)
    } else {
        rect.center()
    };
    let size = super::tokens::icon::SMALL;
    icon.image(palette.accent, size).paint_at(
        ui,
        egui::Rect::from_center_size(icon_center, Vec2::splat(size)),
    );
    if labels {
        ui.painter().galley(
            pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.accent,
        );
    }
    response
}

/// Show sidebar, Home, Back and Forward: icon buttons that stay in place
/// while they cannot act, faded and deaf to clicks.
fn nav_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    enabled: bool,
    label: &str,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        IconButton::new(icon, label)
            .size(NAV_SIZE)
            .show(ui, palette)
    })
    .inner
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let width = ui.available_width();
    let window_controls = super::window_controls_reservation(
        ui.ctx(),
        app.show_queue_panel || app.show_friends_panel,
        app.show_lyrics_panel,
        width,
    );
    // Where the titlebar used to be: the bar grows upwards into that space and
    // its empty parts drag the window.
    let inset = theme::titlebar_inset(ui.ctx());
    let content_height = theme::TOP_BAR_HEIGHT + inset;
    if crate::window::custom_titlebar() {
        super::titlebar_drag(
            ui,
            egui::Rect::from_min_size(
                ui.cursor().min,
                vec2(width, content_height + window_controls.topbar_top),
            ),
        );
    }
    ui.add_space(window_controls.topbar_top);
    ui.allocate_ui_with_layout(
        vec2(width, content_height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add_space(super::widgets::PAGE_PADDING);
            ui.spacing_mut().item_spacing.x = ITEM_SPACING;
            let sidebar_shown = super::sidebar_shown(app, ui.ctx());
            if !sidebar_shown {
                if nav_button(
                    ui,
                    &palette,
                    Icon::PanelLeft,
                    true,
                    super::keys::platform_shortcut(
                        &gettext(locale, "Show sidebar (Ctrl+B)"),
                        &gettext(locale, "Show sidebar (Cmd+B)"),
                    ),
                )
                .clicked()
                {
                    app.actions.push(show_sidebar(app));
                }
                ui.add_space(2.0);
            }
            if !sidebar_shown
                && nav_button(ui, &palette, Icon::House, true, &gettext(locale, "Home")).clicked()
            {
                app.actions.push(Action::Open(Page::Home));
            }
            if nav_button(
                ui,
                &palette,
                Icon::ChevronLeft,
                app.can_go_back(),
                &gettext(locale, "Back"),
            )
            .clicked()
            {
                app.actions.push(Action::Back);
            }
            if nav_button(
                ui,
                &palette,
                Icon::ChevronRight,
                app.can_go_forward(),
                &gettext(locale, "Forward"),
            )
            .clicked()
            {
                app.actions.push(Action::Forward);
            }
            ui.add_space(8.0);

            // The badges sit at the right end but grow with their text, so
            // measure them here, before the search field takes its share.
            let device_galley = app.now_playing().filter(|now| !now.local).map(|now| {
                let label = match now.device_name {
                    Some(device) => {
                        // Translators: {device} is the name of the device playing the music.
                        gettext(locale, "Playing on {device}").replace("{device}", &device)
                    }
                    None => gettext(locale, "Playing on another device").into_owned(),
                };
                ui.painter()
                    .layout_no_wrap(label, theme::medium(12.5), palette.accent)
            });
            // Ask once, so the bar reserves room for exactly the spinner it
            // then draws.
            let busy = app
                .backend
                .activity()
                .busy(std::time::Duration::from_millis(1000));
            let badges =
                |labels: bool| badge_width(device_galley.as_ref(), DEVICE_BADGE_PADDING, labels);
            let controls = RIGHT_CONTROLS_WIDTH
                + if busy {
                    SPINNER_SIZE + ITEM_SPACING
                } else {
                    0.0
                };

            let search_room = (ui.available_width() - window_controls.topbar_width).max(0.0);
            // What sits before the field: the page padding, the navigation
            // buttons, and any window buttons. The panels beside the page
            // keep room for it (`least_width`).
            let lead = width - ui.available_width() + window_controls.topbar_width;
            ui.ctx().data_mut(|data| data.insert_temp(lead_id(), lead));
            let fit = topbar_fit(search_room, controls, badges(true), badges(false));
            let search_width = fit.search;
            let id = egui::Id::new("global-search");
            let before = app.search.query.clone();
            let response = super::widgets::floating_search_field(
                ui,
                &palette,
                app.locale,
                id,
                &mut app.search.query,
                &gettext(locale, "What do you want to play?"),
                search_width,
            );
            if app.search.focus_requested {
                app.search.focus_requested = false;
                response.request_focus();
            }
            // Clear empties the field and hands it focus in the same frame.
            // Neither should leave the page: only typing a query does.
            let cleared = app.search.query.is_empty() && !before.is_empty();
            if response.gained_focus() && !cleared && !matches!(app.page(), Page::Search) {
                app.actions.push(Action::Open(Page::Search));
            }
            if app.search.query != before {
                app.search.typed_at = Some(std::time::Instant::now());
                if !cleared && !matches!(app.page(), Page::Search) {
                    app.actions.push(Action::Open(Page::Search));
                }
            }
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                let query = app.search.query.clone();
                app.actions.push(Action::Search(query));
            }
            if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                response.surrender_focus();
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(window_controls.topbar_width);
                ui.add_space(super::widgets::PAGE_PADDING);
                account(app, ui);
                // A quiet spinner once the app has been talking to Spotify for a
                // while, long enough that fast requests never flash it.
                if busy {
                    theme::spinner(ui, SPINNER_SIZE, palette.secondary)
                        .on_hover_text(gettext(locale, "Waiting for Spotify…").as_ref());
                }
                // Where playback is.
                if let Some(galley) = device_galley {
                    let device = galley.text().to_owned();
                    let response = badge(
                        ui,
                        &palette,
                        Icon::Speaker,
                        galley,
                        DEVICE_BADGE_PADDING,
                        fit.labels,
                    );
                    // Without its label the badge still has to say where
                    // playback went.
                    let response = if fit.labels {
                        response
                    } else {
                        response.on_hover_text(device)
                    };
                    if response.clicked() {
                        app.actions.push(Action::ToggleDevicesPopup);
                    }
                }
            });
        },
    );
}

/// The account avatar and its menu, which also holds Settings, the
/// Winamp mini player, MilkDrop and, when there is one, a newer release.
/// A newer release shows as a dot on the avatar: most people never visit
/// a releases page, so the app says so, quietly, until they do.
fn account(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let (name, avatar) = app
        .user
        .as_ref()
        .map(|user| {
            (
                user.name().to_string(),
                pick_image(&user.images, 64).map(str::to_string),
            )
        })
        .unwrap_or_default();
    let update = app.update.clone();
    let mut label = if name.is_empty() {
        gettext(locale, "Account").into_owned()
    } else {
        name.clone()
    };
    if let Some(update) = &update {
        // Translators: {version} is a version number such as 1.2.0.
        let notice = gettext(locale, "Version {version} is available.")
            .replace("{version}", &update.version);
        label = format!("{label}. {notice}");
    }
    let art = app.backend.art();
    let response = AvatarButton::new(&label)
        .size(AVATAR_SIZE)
        .notice(update.is_some())
        .show(ui, &palette, |ui, picture| match avatar.as_deref() {
            Some(url) => super::widgets::paint_cover(
                ui,
                &palette,
                Some(url),
                picture,
                picture.height() / 2.0,
                Icon::User,
                Some(art),
            ),
            None => {
                let initial = name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string();
                ui.painter().circle_filled(
                    picture.center(),
                    picture.height() / 2.0,
                    palette.accent,
                );
                ui.painter().text(
                    picture.center(),
                    egui::Align2::CENTER_CENTER,
                    initial,
                    theme::bold(14.0),
                    palette.on_accent,
                );
            }
        });
    let commands = [
        (
            Icon::Settings,
            gettext(locale, "Settings"),
            super::keys::SETTINGS_SHORTCUT,
            Action::Open(Page::Settings),
        ),
        (
            Icon::Keyboard,
            gettext(locale, "Keyboard shortcuts"),
            super::keys::SHORTCUTS_SHORTCUT,
            Action::ShowDialog(crate::model::Dialog::Shortcuts),
        ),
        (
            Icon::PictureInPicture,
            gettext(locale, "Winamp mini player"),
            super::keys::WINAMP_SHORTCUT,
            Action::ToggleWinampWindow,
        ),
        (
            Icon::Sparkles,
            gettext(locale, "MilkDrop visualiser"),
            super::keys::MILKDROP_SHORTCUT,
            Action::ToggleWinampMilkdrop,
        ),
    ];
    egui::Popup::menu(&response)
        .frame(super::widgets::menu_frame(ui.ctx(), &palette))
        .align(egui::RectAlign::BOTTOM_END)
        .show(|ui| {
            // As wide as the longest label beside its shortcut.
            let measure = |text: &str, font| {
                ui.painter()
                    .layout_no_wrap(text.to_owned(), font, palette.text)
                    .size()
                    .x
            };
            let width = commands
                .iter()
                .map(|(_, label, shortcut, _)| {
                    measure(label, theme::regular(13.5)) + measure(shortcut, theme::regular(12.0))
                })
                .fold(0.0_f32, f32::max)
                + MENU_ITEM_CHROME;
            ui.set_width(
                width
                    .max(MENU_WIDTH)
                    .min(ui.ctx().content_rect().width() - 24.0),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                theme::text(ui, &name, theme::semibold(14.0), palette.text);
            });
            if let Some(product) = app.user.as_ref().and_then(|user| user.product.clone()) {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    theme::text(
                        ui,
                        capitalize(&product),
                        theme::regular(12.0),
                        palette.secondary,
                    );
                });
            }
            super::widgets::menu_separator(ui, &palette);
            if let Some(update) = &update {
                let label = match &app.update_download {
                    crate::updates::DownloadState::Ready(_) => {
                        gettext(locale, "Update ready").into_owned()
                    }
                    crate::updates::DownloadState::Downloading { .. } => {
                        gettext(locale, "Downloading update…").into_owned()
                    }
                    _ => {
                        // Translators: {version} is a version number such as 1.2.0.
                        gettext(locale, "Update to {version}").replace("{version}", &update.version)
                    }
                };
                if super::widgets::menu_item(ui, &palette, Some(Icon::CircleArrowDown), &label) {
                    app.actions.push(Action::ShowUpdate);
                }
                super::widgets::menu_separator(ui, &palette);
            }
            for (icon, label, shortcut, action) in commands {
                if super::widgets::menu_item_shortcut(ui, &palette, Some(icon), &label, shortcut) {
                    app.actions.push(action);
                }
            }
            super::widgets::menu_separator(ui, &palette);
            if super::widgets::menu_item(
                ui,
                &palette,
                Some(Icon::LogOut),
                &gettext(locale, "Sign out"),
            ) {
                app.actions.push(Action::SignOut);
            }
        });
}

/// The account menu's least width, and what an item adds around its label
/// and shortcut: the icon, the gaps and the padding at both ends.
const MENU_WIDTH: f32 = 220.0;
const MENU_ITEM_CHROME: f32 = 10.0 + 26.0 + 12.0 + 10.0 + 8.0;

/// What Show sidebar does: turn the sidebar on or, when it is on but a
/// right panel leaves it no room, close that panel to make some.
fn show_sidebar(app: &App) -> Action {
    if !app.settings.sidebar_visible {
        Action::ToggleSidebar
    } else if app.show_queue_panel {
        Action::ToggleQueuePanel
    } else if app.show_lyrics_panel {
        Action::ToggleLyricsPanel
    } else {
        Action::ToggleFriendsPanel
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod topbar_fit_tests {
    use super::*;

    // What the badge measures on a bar showing "Playing on MacBook de Luis",
    // including the spacing before it.
    const DEVICE: f32 = ITEM_SPACING + 176.0;
    // Collapsed, the badge is a square chip as tall as its text.
    const CHIP: f32 = ITEM_SPACING + 15.0 + BADGE_PADDING_Y;

    /// The narrowest bar the app can produce: a 760 px window, its sidebar,
    /// and the navigation buttons all taken out.
    const NARROWEST_BAR: f32 = 398.0;

    fn right_end(room: f32, labelled: f32, icons: f32) -> f32 {
        let fit = topbar_fit(room, RIGHT_CONTROLS_WIDTH, labelled, icons);
        let badges = if fit.labels { labelled } else { icons };
        RIGHT_CONTROLS_WIDTH + badges - (room - fit.search)
    }

    #[test]
    fn a_wide_bar_keeps_the_field_it_always_had() {
        let fit = topbar_fit(2000.0, RIGHT_CONTROLS_WIDTH, DEVICE, CHIP);
        assert_eq!(fit.search, SEARCH_MAX);
        assert!(fit.labels);
        // Half the room, as before, while half still fits.
        let fit = topbar_fit(700.0, RIGHT_CONTROLS_WIDTH, 0.0, 0.0);
        assert_eq!(fit.search, 350.0);
    }

    #[test]
    fn the_right_end_never_reaches_over_the_search_field() {
        let mut room = NARROWEST_BAR;
        while room <= 2400.0 {
            for (labelled, icons) in [(0.0, 0.0), (DEVICE, CHIP)] {
                let over = right_end(room, labelled, icons);
                assert!(
                    over <= 0.0,
                    "the badge overlaps the field by {over} px on a {room} px bar"
                );
            }
            room += 1.0;
        }
    }

    #[test]
    fn a_right_panel_can_narrow_search_after_the_badge_collapses() {
        let room = RIGHT_CONTROLS_WIDTH + CHIP + 100.0;
        let fit = topbar_fit(room, RIGHT_CONTROLS_WIDTH, DEVICE, CHIP);
        assert!(!fit.labels);
        assert_eq!(fit.search, 100.0);
        assert_eq!(right_end(room, DEVICE, CHIP), 0.0);
    }

    /// A tiling window manager can make the window narrower than its
    /// minimum, and a right panel then leaves the page less than the bar
    /// needs (B1 of the UX audit). With Settings, the Winamp mini player
    /// and MilkDrop in the account menu, the avatar and the collapsed
    /// badge still sit clear of the narrowest field there.
    #[test]
    fn a_bar_below_its_least_width_keeps_the_right_end_off_the_field() {
        let mut room = RIGHT_CONTROLS_WIDTH + CHIP + SEARCH_MIN;
        while room < NARROWEST_BAR {
            let over = right_end(room, DEVICE, CHIP);
            assert!(over <= 0.0, "{over} px over the field on a {room} px bar");
            room += 1.0;
        }
    }

    #[test]
    fn a_narrow_bar_trades_the_badge_label_for_its_icon() {
        assert!(topbar_fit(1400.0, RIGHT_CONTROLS_WIDTH, DEVICE, CHIP).labels);
        // The 1080 px window of the report that started this.
        assert!(topbar_fit(952.0, RIGHT_CONTROLS_WIDTH, DEVICE, CHIP).labels);
        assert!(
            !topbar_fit(
                RIGHT_CONTROLS_WIDTH + DEVICE,
                RIGHT_CONTROLS_WIDTH,
                DEVICE,
                CHIP
            )
            .labels
        );
    }

    /// At the least width the panels leave it, the bar still holds the
    /// spinner and the badge beside the narrowest field (#624).
    #[test]
    fn the_least_width_holds_every_control_beside_the_field() {
        let lead = LEAD_GUESS;
        let room = least_width_after(lead) - lead;
        let controls = RIGHT_CONTROLS_WIDTH + SPINNER_SIZE + ITEM_SPACING;
        let fit = topbar_fit(room, controls, DEVICE, CHIP);
        assert!(!fit.labels);
        assert_eq!(fit.search, SEARCH_MIN);
        assert!(controls + CHIP + fit.search <= room);
    }

    #[test]
    fn the_field_stays_readable_however_tight_the_bar_gets() {
        let mut room = NARROWEST_BAR;
        while room <= 2400.0 {
            let fit = topbar_fit(room, RIGHT_CONTROLS_WIDTH, DEVICE, CHIP);
            assert!(fit.search >= SEARCH_FLOOR, "field is {} px", fit.search);
            assert!(fit.search <= SEARCH_MAX);
            room += 1.0;
        }
    }
}
