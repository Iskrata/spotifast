//! The now-playing bar along the bottom of the window.

use egui::{Align, Color32, Layout, Margin, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

use super::{motion, tokens};
use crate::app::{App, NowPlaying};
use crate::i18n::gettext;
use crate::model::{Action, DragTrack, Page};
use crate::player::RepeatMode;
use crate::theme::{self, Icon};
use crate::util;

use super::buttons::{IconButton, IconSize, PlayDisc};
use super::widgets::{SliderEvent, thin_slider};

/// How much of the playing art's tint the bar's fill carries.
const TINT_STRENGTH: f32 = 0.12;
/// How long the bar takes to cross over to a new song's tint.
const TINT_FADE_SECONDS: f32 = 0.45;
const TINT_SESSION_ID: &str = "player-bar-tint-session";

/// Forget this bar's animation session while the sign-in screen is shown.
pub(crate) fn end_tint_session(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<u64>(egui::Id::new(TINT_SESSION_ID)));
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let glass = super::glass::on(ui.ctx());
    // As glass, the art itself colours the bar.
    let fill = if glass {
        palette.panel
    } else {
        eased_fill(ui.ctx(), palette.panel, app.now_playing_tint())
    };
    let floating = super::glass::Floating::begin(ui);
    let bar = egui::Panel::bottom("player-bar")
        .exact_size(theme::PLAYER_BAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            super::glass::panel_frame(ui.ctx(), &palette, fill)
                .inner_margin(Margin::symmetric(16, 0)),
        )
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let now = app.now_playing();
            // As glass, the bar's own edge sets it apart.
            if !glass {
                ui.painter().hline(
                    rect.x_range(),
                    rect.top() + 0.5,
                    egui::Stroke::new(1.0, palette.outline),
                );
            }
            let width = rect.width();
            let side = (width * 0.3).clamp(200.0, 420.0);
            let cy = rect.center().y;
            let left = Rect::from_min_max(rect.min, pos2(rect.left() + side, rect.bottom()));
            // The right side never gets less than its controls need; the
            // transport stays centred by giving up the same on both sides.
            let reach = side.max(RIGHT_LEAST_WIDTH);
            let center = Rect::from_min_max(
                pos2(rect.left() + reach, rect.top()),
                pos2(rect.right() - reach, rect.bottom()),
            );

            // egui's cross-axis centring is unreliable across nested layouts of
            // mixed heights, so each region is placed in an explicit band that
            // is sized to its content and centred on the bar's midline.
            now_playing_block(app, ui, left, now.as_ref());

            transport(app, ui, now.as_ref(), center);

            let right_band =
                Rect::from_min_size(pos2(rect.right() - reach, cy - 16.0), vec2(reach, 32.0));
            let mut right_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(right_band)
                    .layout(Layout::right_to_left(Align::Center)),
            );
            extras(app, &mut right_ui, now.as_ref());
        });
    floating.finish(ui, &palette, bar.response.rect);
}

/// Ease the final fill's RGB. Untinted custom panels keep their alpha while
/// the colour returns to the panel colour.
fn eased_fill(ctx: &egui::Context, panel: Color32, tint: Option<Color32>) -> Color32 {
    let target = tint.map_or(panel, |tint| super::blend(panel, tint, TINT_STRENGTH));
    // Color32 stores premultiplied RGB, so interpolate unmultiplied channels.
    let [r, g, b, _] = target.to_srgba_unmultiplied();
    // A new pass after sign-out gets new ids without clearing other animations.
    let pass = ctx.cumulative_pass_nr();
    let session = ctx.data_mut(|data| {
        *data.get_temp_mut_or_insert_with(egui::Id::new(TINT_SESSION_ID), || pass)
    });
    let channel = |axis: &'static str, value: u8| {
        ctx.animate_value_with_time(
            egui::Id::new(("player-bar-tint", session, axis)),
            f32::from(value),
            TINT_FADE_SECONDS,
        )
        .round() as u8
    };
    let eased = [channel("r", r), channel("g", g), channel("b", b)];
    if eased == [r, g, b] {
        target
    } else {
        Color32::from_rgba_unmultiplied(eased[0], eased[1], eased[2], target.a())
    }
}

fn now_playing_block(app: &mut App, ui: &mut egui::Ui, region: Rect, now: Option<&NowPlaying>) {
    let palette = app.palette;
    let cy = region.center().y;
    let cover_rect = Rect::from_min_size(pos2(region.left() + 4.0, cy - 28.0), Vec2::splat(56.0));

    let Some(now) = now else {
        super::widgets::paint_cover(ui, &palette, None, cover_rect, 6.0, Icon::Music, None);
        let text_left = cover_rect.right() + 12.0;
        let text_rect = Rect::from_min_size(
            pos2(text_left, cy - 17.0),
            vec2((region.right() - text_left - 8.0).max(40.0), 34.0),
        );
        let mut text_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(text_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        text_ui.spacing_mut().item_spacing.y = 2.0;
        theme::text(
            &mut text_ui,
            gettext(app.locale, "Nothing playing"),
            theme::medium(14.0),
            palette.secondary,
        );
        theme::text(
            &mut text_ui,
            gettext(app.locale, "Pick a song, album, or playlist"),
            theme::regular(12.0),
            palette.dim,
        );
        return;
    };

    // A new song cross-fades in over the one before.
    let (arrived, before) = song_change(ui.ctx(), now);
    let paint_art = |ui: &egui::Ui, art: Option<&str>| {
        super::widgets::paint_cover(
            ui,
            &palette,
            art,
            cover_rect,
            tokens::points(tokens::radius::ROW),
            Icon::Music,
            Some(app.backend.art()),
        );
    };
    if let Some(before) = &before {
        paint_art(ui, before.art.as_deref());
    }
    let mut art_ui = ui.new_child(UiBuilder::new().max_rect(cover_rect));
    art_ui.multiply_opacity(arrived);
    paint_art(&art_ui, now.art_small.as_deref().or(now.art_url.as_deref()));
    let song = app.now_playing_item();
    let drag_sense = if song.is_some() {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let cover_response = ui
        .interact(cover_rect, egui::Id::new("now-playing-cover"), drag_sense)
        .on_hover_cursor(if song.is_some() {
            egui::CursorIcon::Grab
        } else {
            egui::CursorIcon::Default
        });
    // Hovering the cover offers to dock the art large at the sidebar's
    // bottom, the way Spotify expands it. (#92)
    let art_available = now.art_url.is_some() || now.art_small.is_some();
    let expand_rect = super::buttons::overlay_rect(cover_rect, 2.0);
    let offer_expand =
        art_available && !app.settings.art_expanded && super::sidebar_shown(app, ui.ctx());
    let over_expand = offer_expand && ui.rect_contains_pointer(expand_rect);
    if cover_response.clicked() && !over_expand {
        open_playing(app, now);
    }
    if offer_expand && (cover_response.hovered() || over_expand) {
        let expand = super::buttons::overlay(
            ui,
            &palette,
            expand_rect,
            egui::Id::new("now-playing-art-expand"),
            Icon::ChevronUp,
        );
        if expand.clicked() {
            app.settings.art_expanded = true;
            app.actions.push(Action::SettingsChanged);
        }
    }
    let heart_width = if now.is_episode { 0.0 } else { 42.0 };
    let text_left = cover_rect.right() + 12.0;
    let text_width = (region.right() - text_left - heart_width).max(40.0);
    let text_rect = Rect::from_min_size(pos2(text_left, cy - 18.0), vec2(text_width, 36.0));
    let info_response = ui.interact(text_rect, egui::Id::new("now-playing-info"), drag_sense);
    let mut text_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    text_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
    text_ui.spacing_mut().item_spacing.y = 2.0;
    if let Some(before) = &before {
        paint_words_before(&text_ui, &palette, before, 1.0 - arrived);
        text_ui.multiply_opacity(arrived);
    }
    let title_response = theme::link(&mut text_ui, &now.title, theme::medium(14.0), palette.text);
    if title_response.clicked() {
        open_playing(app, now);
    }
    text_ui.horizontal_top(|ui| {
        byline(ui, app, now, theme::regular(12.0), palette.secondary);
    });
    if (cover_response.drag_started_by(egui::PointerButton::Primary)
        || info_response.drag_started_by(egui::PointerButton::Primary))
        && let Some(item) = &song
    {
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            DragTrack {
                title: item.name().to_string(),
                image: item.image(64).map(str::to_string),
                items: vec![item.clone()],
                from: None,
                source_playlist: None,
            },
        );
    }

    // The playing thing answers the same right-click menu as a table row,
    // from the cover, the empty space around the words, or the words.
    if let Some(item) = song {
        let context = app.editable_context_playlist();
        for response in [&cover_response, &info_response, &title_response] {
            egui::Popup::context_menu(response)
                .frame(super::widgets::menu_frame(ui.ctx(), &palette))
                .show(|ui| super::widgets::item_menu(ui, app, &item, context.as_ref(), None));
        }
    }

    if !now.is_episode {
        // Sit the heart just past the actual text, not at the region's far
        // edge, so it stays visually attached to the title.
        let natural = {
            let title =
                ui.painter()
                    .layout_no_wrap(now.title.clone(), theme::medium(14.0), palette.text);
            let subtitle = ui.painter().layout_no_wrap(
                now.subtitle.clone(),
                theme::regular(12.0),
                palette.secondary,
            );
            title.size().x.max(subtitle.size().x).min(text_width)
        };
        let heart_x = (text_left + natural + 21.0).min(region.right() - 21.0);
        let heart_rect = Rect::from_center_size(pos2(heart_x, cy), Vec2::splat(30.0));
        let mut heart_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(heart_rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        super::widgets::heart_button(&mut heart_ui, app, &now.uri, IconSize::Standard);
    }
}

/// The song the bar showed before the playing one, while it fades out.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    uri: String,
    art: Option<String>,
    title: String,
    subtitle: String,
}

impl Shown {
    fn of(now: &NowPlaying) -> Self {
        Self {
            uri: now.uri.clone(),
            art: now.art_small.clone().or_else(|| now.art_url.clone()),
            title: now.title.clone(),
            subtitle: now.subtitle.clone(),
        }
    }
}

/// How far the playing song has faded in since it took over the bar, over
/// the content motion duration, and the song before it while that lasts.
/// The song the bar opens with is simply there.
fn song_change(ctx: &egui::Context, now: &NowPlaying) -> (f32, Option<Shown>) {
    let id = egui::Id::new("player-bar-song");
    let state = ctx.data(|data| data.get_temp::<(Shown, Option<Shown>)>(id));
    let before = match state {
        Some((shown, before)) if shown.uri == now.uri => before,
        Some((shown, _)) => {
            let before = Some(shown);
            ctx.data_mut(|data| data.insert_temp(id, (Shown::of(now), before.clone())));
            before
        }
        None => {
            ctx.data_mut(|data| data.insert_temp(id, (Shown::of(now), None::<Shown>)));
            None
        }
    };
    let arrived = motion::arrival(
        ctx,
        id.with("arrival"),
        egui::Id::new(&now.uri),
        tokens::motion::CONTENT,
    );
    if arrived >= 1.0 && before.is_some() {
        ctx.data_mut(|data| data.insert_temp(id, (Shown::of(now), None::<Shown>)));
        return (arrived, None);
    }
    (arrived, before)
}

/// The song before's title and byline, where the playing song's go,
/// fading out at `opacity`.
fn paint_words_before(ui: &egui::Ui, palette: &theme::Palette, before: &Shown, opacity: f32) {
    let painter = ui.painter().clone().with_clip_rect(ui.clip_rect());
    let title = painter.layout_no_wrap(
        before.title.clone(),
        theme::medium(14.0),
        palette.text.gamma_multiply(opacity),
    );
    let top = ui.max_rect().left_top();
    let below = top.y + title.size().y + ui.spacing().item_spacing.y;
    painter.galley(top, title, palette.text);
    let subtitle = painter.layout_no_wrap(
        before.subtitle.clone(),
        theme::regular(12.0),
        palette.secondary.gamma_multiply(opacity),
    );
    painter.galley(pos2(top.x, below), subtitle, palette.secondary);
}

/// Opens the playing song's album, or the playing episode's show.
pub(super) fn open_playing(app: &mut App, now: &NowPlaying) {
    if let Some(id) = &now.album_id {
        app.actions.push(Action::Open(Page::Album(id.clone())));
    } else if let Some(id) = &now.show_id {
        app.actions.push(Action::Open(Page::Show(id.clone())));
    }
}

/// The playing song's artists, each opening its page, or the playing
/// episode's show.
pub(super) fn byline(
    ui: &mut egui::Ui,
    app: &mut App,
    now: &NowPlaying,
    font: egui::FontId,
    color: Color32,
) {
    if now.artists.is_empty() {
        if theme::link(ui, &now.subtitle, font, color).clicked()
            && let Some(id) = &now.show_id
        {
            app.actions.push(Action::Open(Page::Show(id.clone())));
        }
    } else {
        super::widgets::artist_links(ui, app, &now.artists, font, color);
    }
}

/// The glass capsule the transport buttons sit in, `width` wide around
/// `center`.
fn transport_capsule(ui: &egui::Ui, palette: &theme::Palette, center: egui::Pos2, width: f32) {
    let height = tokens::disc::TRANSPORT + 2.0 * TRANSPORT_CAPSULE_PADDING;
    let rect = Rect::from_center_size(
        center,
        vec2(width + 2.0 * TRANSPORT_CAPSULE_PADDING, height),
    );
    let corner = tokens::capsule(height);
    // A pane of light set into the bar's glass.
    let fill = palette
        .glass_highlight()
        .gamma_multiply(if palette.dark { 0.25 } else { 0.5 });
    ui.painter().rect_filled(rect, corner, fill);
    ui.painter().add(super::glass::edge(palette, rect, corner));
}

/// The room around the transport buttons inside their capsule.
const TRANSPORT_CAPSULE_PADDING: f32 = 6.0;

fn transport(app: &mut App, ui: &mut egui::Ui, now: Option<&NowPlaying>, region: Rect) {
    let palette = app.palette;
    // Everything here is placed with explicit rects: egui's implicit rows
    // centre each widget in the row height known when it is added, which
    // left earlier icons riding high next to the play disc.
    //
    // The buttons row (36) and the progress row (~15, after a 6px gap) form
    // one cluster, centred as a group in the 88px bar: the buttons sit 8px
    // above the bar's midline and the progress row 23px below it. Measured
    // on screen this puts equal breathing room above and beneath the
    // cluster.
    let cy = region.center().y - 8.0;
    let enabled = now.is_some_and(|now| now.can_control) || app.is_connected();
    let playing = now.is_some_and(|now| now.playing);
    let loading = now.is_some_and(|now| now.loading);
    let shuffle = now.map_or_else(|| app.playing_context_shuffle(), |now| now.shuffle);
    let repeat = now.map(|now| now.repeat).unwrap_or_default();
    // Button widths: standard icon buttons, and the 36-point disc.
    let icon = IconSize::Standard.hit();
    let widths = [icon, icon, 36.0, icon, icon];
    let gap = 10.0;
    let total: f32 = widths.iter().sum::<f32>() + gap * 4.0;
    let mut x = region.center().x - total / 2.0;
    if super::glass::on(ui.ctx()) {
        transport_capsule(ui, &palette, pos2(region.center().x, cy), total);
    }
    let mut slot = |width: f32| {
        let rect = Rect::from_center_size(pos2(x + width / 2.0, cy), vec2(width, 36.0));
        x += width + gap;
        rect
    };
    let centered = |ui: &mut egui::Ui, rect: Rect| {
        ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        )
    };

    let mut cell = centered(ui, slot(widths[0]));
    let shuffle_button = IconButton::new(Icon::Shuffle, &gettext(app.locale, "Shuffle"))
        .active(shuffle)
        .dimmed(!enabled)
        .show(&mut cell, &palette);
    shuffle_button.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            cell.is_enabled(),
            shuffle,
            gettext(app.locale, "Shuffle"),
        )
    });
    if shuffle_button.clicked() {
        app.actions.push(Action::ToggleShuffle);
    }

    let mut cell = centered(ui, slot(widths[1]));
    if IconButton::new(Icon::SkipBackFilled, &gettext(app.locale, "Previous"))
        .dimmed(!enabled)
        .show(&mut cell, &palette)
        .clicked()
    {
        app.actions.push(Action::Previous);
    }

    let disc = slot(widths[2]);
    if loading || app.any_play_pending() {
        ui.painter()
            .circle_filled(disc.center(), 18.0, palette.text);
        let mut cell = centered(ui, disc);
        theme::spinner(&mut cell, 22.0, palette.window);
    } else {
        let mut cell = centered(ui, disc);
        if PlayDisc::transport(&if playing {
            gettext(app.locale, "Pause")
        } else {
            gettext(app.locale, "Play")
        })
        .playing(playing)
        .show(&mut cell, &palette)
        .clicked()
        {
            app.actions.push(Action::TogglePlay);
        }
    }

    let mut cell = centered(ui, slot(widths[3]));
    if IconButton::new(Icon::SkipForwardFilled, &gettext(app.locale, "Next"))
        .dimmed(!enabled)
        .show(&mut cell, &palette)
        .clicked()
    {
        app.actions.push(Action::Next);
    }

    let (repeat_icon, tooltip) = match repeat {
        RepeatMode::Off => (Icon::Repeat, gettext(app.locale, "Repeat")),
        RepeatMode::Context => (Icon::Repeat, gettext(app.locale, "Repeat one")),
        RepeatMode::Track => (Icon::Repeat1, gettext(app.locale, "Repeat off")),
    };
    let mut cell = centered(ui, slot(widths[4]));
    if IconButton::new(repeat_icon, &tooltip)
        .active(repeat != RepeatMode::Off)
        .dimmed(!enabled)
        .show(&mut cell, &palette)
        .clicked()
    {
        app.actions.push(Action::CycleRepeat);
    }

    // Progress row, just below the buttons (disc bottom + 6px gap + half of
    // the time text's line height).
    let row_cy = cy + 31.0;
    let slider_width = (region.width() - 120.0).clamp(120.0, 620.0);
    let (position, duration) = now
        .map(|now| (now.position_ms, now.duration_ms))
        .unwrap_or((0, 0));
    let shown_position = match app.seek_preview {
        Some(fraction) => (fraction * duration as f32) as u32,
        None => position,
    };
    let time_color = if now.is_some() {
        palette.secondary
    } else {
        palette.dim
    };
    let slider_left = region.center().x - slider_width / 2.0;
    ui.painter().text(
        pos2(slider_left - 8.0, row_cy),
        egui::Align2::RIGHT_CENTER,
        util::format_duration_ms(shown_position),
        theme::regular(11.5),
        time_color,
    );
    let slider_rect =
        Rect::from_center_size(pos2(region.center().x, row_cy), vec2(slider_width, 16.0));
    let mut slider_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(slider_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let fraction = if duration > 0 {
        position as f32 / duration as f32
    } else {
        0.0
    };
    match thin_slider(
        &mut slider_ui,
        &palette,
        egui::Id::new("seek-slider"),
        &gettext(app.locale, "Playback position (%)"),
        fraction,
        slider_width,
        None,
    ) {
        SliderEvent::Dragging(value) => app.seek_preview = Some(value),
        SliderEvent::Committed(value) => {
            app.seek_preview = None;
            if duration > 0 {
                app.actions
                    .push(Action::Seek((value * duration as f32) as u32));
            }
        }
        SliderEvent::None => {}
    }
    ui.painter().text(
        pos2(slider_left + slider_width + 8.0, row_cy),
        egui::Align2::LEFT_CENTER,
        util::format_duration_ms(duration),
        theme::regular(11.5),
        time_color,
    );
}

fn extras(app: &mut App, ui: &mut egui::Ui, now: Option<&NowPlaying>) {
    let palette = app.palette;
    ui.spacing_mut().item_spacing.x = tokens::gap::ICONS;
    let volume = now
        .map(|now| now.volume_percent)
        .unwrap_or_else(|| crate::app::volume_to_percent(app.local.volume));
    let shown = match app.volume_preview {
        Some(fraction) => (fraction * 100.0).round() as u8,
        None => volume,
    };
    // Some remote devices, such as a phone playing over Bluetooth, refuse
    // volume changes from other apps: show their volume but don't offer to
    // change it.
    let adjustable = now.is_none_or(|now| now.can_set_volume);
    let controls = ui.add_enabled_ui(adjustable, |ui| {
        match thin_slider(
            ui,
            &palette,
            egui::Id::new("volume-slider"),
            &gettext(app.locale, "Volume (%)"),
            shown as f32 / 100.0,
            volume_width(ui.max_rect().width()),
            Some(0.05),
        ) {
            SliderEvent::Dragging(value) => {
                app.volume_preview = Some(value);
                // Local volume is cheap to apply continuously; remote goes on release.
                if now.is_none_or(|now| now.local) {
                    app.actions
                        .push(Action::PreviewVolume((value * 100.0).round() as u8));
                }
            }
            SliderEvent::Committed(value) => {
                app.volume_preview = None;
                app.actions
                    .push(Action::SetVolume((value * 100.0).round() as u8));
            }
            SliderEvent::None => {}
        }
        let volume_icon = match shown {
            0 => Icon::VolumeX,
            1..=33 => Icon::Volume,
            34..=66 => Icon::Volume1,
            _ => Icon::Volume2,
        };
        if IconButton::new(
            volume_icon,
            &if shown == 0 {
                gettext(app.locale, "Unmute")
            } else {
                gettext(app.locale, "Mute")
            },
        )
        .show(ui, &palette)
        .clicked()
        {
            app.actions.push(Action::ToggleMute);
        }
    });
    if !adjustable {
        ui.interact(
            controls.response.rect,
            egui::Id::new("volume-fixed"),
            egui::Sense::hover(),
        )
        .on_hover_text(gettext(
            app.locale,
            "This device's volume can't be changed from Spotifast",
        ));
    }
    ui.add_space(tokens::gap::GROUP);
    panel_toggles(app, ui, now);
}

/// The four controls that open what sits beside the page, read left to
/// right as Lyrics, Queue, Friend Activity and Devices. Each shows the
/// green dot while its panel or popup is open. The bar lays out right to
/// left, so they are added in reverse.
fn panel_toggles(app: &mut App, ui: &mut egui::Ui, now: Option<&NowPlaying>) {
    let palette = app.palette;
    let locale = app.locale;
    // Devices stays here rather than only in the top bar: the badge there
    // names a device only while another one plays, and this is the one
    // control that always offers the list.
    let remote = now.is_some_and(|now| !now.local);
    let devices = IconButton::new(Icon::Speaker, &gettext(locale, "Connect to a device"))
        .active(remote)
        .show(ui, &palette);
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new(super::devices::BUTTON_RECT_ID), devices.rect)
    });
    if devices.clicked() {
        app.actions.push(Action::ToggleDevicesPopup);
    }
    if IconButton::new(Icon::Users, &gettext(locale, "Friend Activity"))
        .active(app.show_friends_panel)
        .show(ui, &palette)
        .clicked()
    {
        app.actions.push(Action::ToggleFriendsPanel);
    }
    let queue_open = app.show_queue_panel || matches!(app.page(), Page::Queue);
    let queue_button = IconButton::new(Icon::ListMusic, &gettext(locale, "Queue"))
        .active(queue_open)
        .show(ui, &palette);
    if queue_button.clicked() {
        app.actions.push(Action::ToggleQueuePanel);
    }
    // A dragged song dropped on the queue button queues it, same as the
    // "Add to queue" menu item.
    if let Some(track) = queue_button.dnd_release_payload::<DragTrack>() {
        app.actions.push(Action::QueueMany {
            songs: track
                .items
                .iter()
                .map(|item| (item.uri().to_string(), item.name().to_string()))
                .collect(),
        });
    }
    if IconButton::new(Icon::Mic, &gettext(locale, "Lyrics"))
        .active(app.show_lyrics_panel)
        .show(ui, &palette)
        .clicked()
    {
        app.actions.push(Action::ToggleLyricsPanel);
    }
}

/// The width of the panel toggles: four standard icon buttons.
const PANEL_TOGGLES_WIDTH: f32 = 4.0 * tokens::hit::STANDARD + 3.0 * tokens::gap::ICONS;
/// The volume slider's width, and the least it shrinks to in a narrow bar.
const VOLUME_WIDTH: f32 = 92.0;
const VOLUME_MIN_WIDTH: f32 = 56.0;

/// The right side without its volume slider: the toggles, then Mute.
const RIGHT_FIXED_WIDTH: f32 =
    PANEL_TOGGLES_WIDTH + tokens::gap::GROUP + tokens::hit::STANDARD + tokens::gap::ICONS;
/// The least the right side takes, with the slider at its narrowest.
const RIGHT_LEAST_WIDTH: f32 = RIGHT_FIXED_WIDTH + VOLUME_MIN_WIDTH;

/// How wide the volume slider is when the bar's right side is `side`
/// points wide: full width while the toggles, Mute and slider fit, and
/// narrower, down to a usable minimum, when they would not.
fn volume_width(side: f32) -> f32 {
    (side - RIGHT_FIXED_WIDTH).clamp(VOLUME_MIN_WIDTH, VOLUME_WIDTH)
}

#[cfg(test)]
mod player_bar_tint_tests {
    use super::*;
    use crate::theme::Palette;

    /// Run one frame at `time` and report the bar's actual fill.
    fn frame(ctx: &egui::Context, time: f64, panel: Color32, tint: Option<Color32>) -> Color32 {
        let mut fill = Color32::PLACEHOLDER;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |ui| fill = eased_fill(ui.ctx(), panel, tint),
        );
        output.textures_delta.clear();
        fill
    }

    #[test]
    fn a_song_without_art_preserves_translucent_panel() {
        for panel in [
            Palette::dark().panel,
            Palette::light().panel,
            Color32::from_rgba_unmultiplied(80, 120, 180, 128),
        ] {
            let ctx = egui::Context::default();
            assert_eq!(frame(&ctx, 0.0, panel, None), panel);
            let art = Color32::from_rgb(200, 40, 90);
            frame(&ctx, 0.1, panel, Some(art));
            frame(&ctx, 0.6, panel, Some(art));
            assert_eq!(frame(&ctx, 0.7, panel, None).a(), panel.a());
            assert_eq!(frame(&ctx, 1.2, panel, None), panel);
        }
    }

    #[test]
    fn the_first_frame_shows_the_tint_without_fading_in() {
        let ctx = egui::Context::default();
        let panel = Palette::dark().panel;
        let art = Color32::from_rgb(200, 40, 90);
        assert_eq!(
            frame(&ctx, 0.0, panel, Some(art)),
            super::super::blend(panel, art, TINT_STRENGTH)
        );
    }

    #[test]
    fn first_art_after_an_untinted_frame_fades_in() {
        let ctx = egui::Context::default();
        let panel = Palette::dark().panel;
        let art = Color32::from_rgb(220, 140, 160);
        let target = super::super::blend(panel, art, TINT_STRENGTH);
        let fade = f64::from(TINT_FADE_SECONDS);

        assert_eq!(frame(&ctx, 0.0, panel, None), panel);
        assert_eq!(frame(&ctx, 0.1, panel, Some(art)), panel);
        let middle = frame(&ctx, 0.1 + fade / 2.0, panel, Some(art));
        assert_ne!(middle, panel);
        assert_ne!(middle, target);
        assert_eq!(frame(&ctx, 0.1 + fade, panel, Some(art)), target);
    }

    #[test]
    fn changing_songs_mid_fade_continues_from_the_visible_colour() {
        let ctx = egui::Context::default();
        let panel = Palette::dark().panel;
        let first = Color32::from_rgb(20, 40, 60);
        let second = Color32::from_rgb(220, 140, 160);
        let third = Color32::from_rgb(40, 230, 30);
        let first_fill = super::super::blend(panel, first, TINT_STRENGTH);
        let second_fill = super::super::blend(panel, second, TINT_STRENGTH);
        let third_fill = super::super::blend(panel, third, TINT_STRENGTH);
        let fade = f64::from(TINT_FADE_SECONDS);

        assert_eq!(frame(&ctx, 0.0, panel, Some(first)), first_fill);
        assert_eq!(frame(&ctx, 0.05, panel, Some(second)), first_fill);
        let middle = frame(&ctx, 0.2, panel, Some(second));
        assert_ne!(middle, first_fill);
        assert_ne!(middle, second_fill);
        assert_eq!(frame(&ctx, 0.2, panel, Some(third)), middle);
        assert_ne!(frame(&ctx, 0.2 + fade / 2.0, panel, Some(third)), middle);
        assert_eq!(frame(&ctx, 0.2 + fade, panel, Some(third)), third_fill);
    }

    #[test]
    fn a_new_session_does_not_reuse_the_previous_tint() {
        let ctx = egui::Context::default();
        let panel = Palette::dark().panel;
        let first = Color32::from_rgb(20, 40, 60);
        let second = Color32::from_rgb(220, 140, 160);
        let next_session = Color32::from_rgb(40, 230, 30);

        frame(&ctx, 0.0, panel, Some(first));
        frame(&ctx, 0.1, panel, Some(second));
        assert_ne!(
            frame(&ctx, 0.2, panel, Some(second)),
            super::super::blend(panel, second, TINT_STRENGTH)
        );

        end_tint_session(&ctx);
        assert_eq!(
            frame(&ctx, 0.21, panel, Some(next_session)),
            super::super::blend(panel, next_session, TINT_STRENGTH)
        );

        end_tint_session(&ctx);
        assert_eq!(frame(&ctx, 0.22, panel, None), panel);
    }

    #[test]
    fn the_volume_slider_narrows_before_the_toggles_crowd_the_seek_bar() {
        assert_eq!(volume_width(420.0), VOLUME_WIDTH);
        // The narrowest window's right side.
        let narrow = volume_width(260.0);
        assert!(
            (VOLUME_MIN_WIDTH..VOLUME_WIDTH).contains(&narrow),
            "{narrow}"
        );
        assert_eq!(volume_width(RIGHT_LEAST_WIDTH), VOLUME_MIN_WIDTH);
        assert_eq!(volume_width(0.0), VOLUME_MIN_WIDTH);
    }
}
