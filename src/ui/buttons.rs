//! The interface's buttons: one icon button in three sizes, three tiers
//! of text button, and the Play disc. Every control draws through here,
//! so its sizes, states and focus ring agree everywhere.

use egui::{Color32, CornerRadius, Response, Sense, Stroke, Ui, Vec2};

use super::{motion, tokens};
use crate::theme::{self, Icon, Palette};

/// How large an icon button is: its icon and the square it answers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSize {
    /// A 16-point icon in 28: rows, menus, the dense corners of panels.
    Compact,
    /// A 20-point icon in 32: toolbars and panel headers.
    Standard,
    /// A 24-point icon in 40: the action row under a page's header.
    Large,
}

impl IconSize {
    pub const fn icon(self) -> f32 {
        match self {
            Self::Compact => tokens::icon::SMALL,
            Self::Standard => tokens::icon::MEDIUM,
            Self::Large => tokens::icon::LARGE,
        }
    }

    pub const fn hit(self) -> f32 {
        match self {
            Self::Compact => tokens::hit::COMPACT,
            Self::Standard => tokens::hit::STANDARD,
            Self::Large => tokens::hit::LARGE,
        }
    }
}

/// A frameless icon control. Hovering lays a circle under the icon and
/// lifts its colour; a control that is on shows a green icon with a dot
/// under it. Its label is required: it is the tooltip and the name a
/// screen reader reads.
#[must_use = "an icon button does nothing until shown"]
pub struct IconButton<'a> {
    icon: Icon,
    label: &'a str,
    size: IconSize,
    active: bool,
    dimmed: bool,
    tint: Option<Color32>,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            icon,
            label,
            size: IconSize::Standard,
            active: false,
            dimmed: false,
            tint: None,
        }
    }

    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    /// A toggle that is on, or a panel that is open: green, with a dot.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Faded at rest while what it controls cannot answer, such as the
    /// transport with nothing to play. It still responds, and lifts on hover.
    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    /// An icon whose own colour carries meaning, such as a filled heart or
    /// a saved album's check. It keeps the colour on hover and shows no dot.
    pub fn tint(mut self, tint: impl Into<Option<Color32>>) -> Self {
        self.tint = tint.into();
        self
    }

    pub fn show(self, ui: &mut Ui, palette: &Palette) -> Response {
        let edge = self.size.hit();
        let (response, lift) = round_control(ui, palette, edge, self.label);
        let rect = response.rect;
        if ui.is_rect_visible(rect) {
            let enabled = ui.is_enabled();
            // The colour at rest, and the one hover and focus lift it to.
            let (rest, lifted) = if !enabled {
                (palette.dim, palette.dim)
            } else if let Some(tint) = self.tint {
                (tint, tint)
            } else if self.active {
                (palette.accent, palette.accent_hover)
            } else if self.dimmed {
                (palette.dim, palette.text)
            } else {
                (palette.secondary, palette.text)
            };
            let color = motion::mix(rest, lifted, lift);
            let size = self.size.icon();
            let icon_rect = egui::Rect::from_center_size(
                rect.center() + theme::play_glyph_offset(self.icon, size),
                Vec2::splat(size),
            );
            self.icon.image(color, size).paint_at(ui, icon_rect);
            if self.active {
                let gap = (edge - size) / 4.0;
                ui.painter().circle_filled(
                    egui::pos2(rect.center().x, rect.bottom() - gap),
                    ACTIVE_DOT / 2.0,
                    color,
                );
            }
        }
        response.on_hover_text(self.label)
    }
}

/// The frame every round control shares: an `edge`-point square that
/// answers to clicks, named `label` for a screen reader, with the hover
/// and press circle under its content and a round focus ring. Returns the
/// response and how far hover or focus has lifted the control, from 0 to 1.
fn round_control(ui: &mut Ui, palette: &Palette, edge: f32, label: &str) -> (Response, f32) {
    debug_assert!(!label.is_empty(), "a round control needs a label");
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(edge), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let enabled = ui.is_enabled();
    let feedback = Feedback::of(
        ui,
        &response,
        enabled && (response.hovered() || response.has_focus()),
    );
    if ui.is_rect_visible(rect) && feedback.shown() {
        let fill = feedback.fill(
            Color32::TRANSPARENT,
            palette.surface_hover,
            palette.surface_active,
        );
        ui.painter().circle_filled(rect.center(), edge / 2.0, fill);
    }
    focus_ring(ui, &response, CornerRadius::same(tokens::radius::ROUND));
    (response, feedback.hover)
}

/// How far hover and press have come on a control, each easing from 0 to
/// 1 at [`tokens::motion::FAST`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feedback {
    pub hover: f32,
    pub press: f32,
}

impl Feedback {
    /// The feedback for `response`, lifted while `hovered` and pressed
    /// while the pointer holds it down.
    pub fn of(ui: &Ui, response: &Response, hovered: bool) -> Self {
        let pressed = ui.is_enabled() && response.is_pointer_button_down_on();
        Self {
            hover: motion::hover(ui.ctx(), response.id.with("hover"), hovered),
            press: motion::hover(ui.ctx(), response.id.with("press"), pressed),
        }
    }

    /// Whether anything shows yet.
    pub fn shown(self) -> bool {
        self.hover > 0.0 || self.press > 0.0
    }

    /// The fill between `rest`, `hovered` and `pressed`.
    pub fn fill(self, rest: Color32, hovered: Color32, pressed: Color32) -> Color32 {
        motion::mix(motion::mix(rest, hovered, self.hover), pressed, self.press)
    }
}

/// The account's picture in the frame of an [`IconButton`] of the same
/// size, so it hovers, presses and takes focus the way the buttons beside
/// it do. Something waiting in the account menu, such as an update, shows
/// as a green dot on it.
#[must_use = "an avatar button does nothing until shown"]
pub struct AvatarButton<'a> {
    label: &'a str,
    size: IconSize,
    notice: bool,
}

impl<'a> AvatarButton<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            size: IconSize::Standard,
            notice: false,
        }
    }

    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    /// Something in the account menu waits for the user.
    pub fn notice(mut self, notice: bool) -> Self {
        self.notice = notice;
        self
    }

    /// The picture's diameter in a button of `size`: the hover circle
    /// shows as a ring around it.
    pub const fn diameter(size: IconSize) -> f32 {
        size.hit() - AVATAR_INSET
    }

    /// Shows the button; `paint` draws the picture into the square it is
    /// given.
    pub fn show(
        self,
        ui: &mut Ui,
        palette: &Palette,
        paint: impl FnOnce(&mut Ui, egui::Rect),
    ) -> Response {
        let (response, _) = round_control(ui, palette, self.size.hit(), self.label);
        let rect = response.rect;
        if ui.is_rect_visible(rect) {
            let picture =
                egui::Rect::from_center_size(rect.center(), Vec2::splat(Self::diameter(self.size)));
            paint(ui, picture);
            if self.notice {
                let center = picture.right_top() + Vec2::new(-NOTICE_DOT / 4.0, NOTICE_DOT / 4.0);
                ui.painter().circle(
                    center,
                    NOTICE_DOT / 2.0,
                    palette.accent,
                    Stroke::new(2.0, palette.window),
                );
            }
        }
        response.on_hover_text(self.label)
    }
}

/// The diameter of the dot under a control that is on.
const ACTIVE_DOT: f32 = 4.0;
/// How much narrower an avatar is than its button.
const AVATAR_INSET: f32 = 8.0;
/// The dot on an avatar that has something waiting.
const NOTICE_DOT: f32 = 10.0;

/// Makes keyboard focus visible around a control, following its own shape
/// (`corner` of the control's rect), without changing its layout.
pub fn focus_ring(ui: &Ui, response: &Response, corner: CornerRadius) {
    if response.has_focus() {
        let rect = response.rect.expand(2.0);
        // Round shapes keep their roundness as the ring grows.
        let grow = |radius: u8| {
            if radius == 0 {
                0
            } else {
                radius.saturating_add(2)
            }
        };
        let corner = CornerRadius {
            nw: grow(corner.nw),
            ne: grow(corner.ne),
            sw: grow(corner.sw),
            se: grow(corner.se),
        };
        ui.painter().rect_stroke(
            rect,
            corner,
            Stroke::new(2.0, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Outside,
        );
    }
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
}

/// The three kinds of text button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tier {
    /// The one green action a view leads with.
    Primary,
    /// Every other labelled action: a grey capsule.
    Secondary,
    /// One choice among several: grey, or inverted while chosen.
    Chip { selected: bool },
}

/// Metrics shared by every text button.
const TEXT_SIZE: f32 = 13.0;
const TEXT_ICON: f32 = tokens::icon::SMALL;
const TEXT_ICON_GAP: f32 = 6.0;

fn padding(tier: Tier) -> f32 {
    match tier {
        Tier::Primary => 16.0,
        Tier::Secondary | Tier::Chip { .. } => 14.0,
    }
}

fn font(tier: Tier) -> egui::FontId {
    match tier {
        Tier::Primary => theme::semibold(TEXT_SIZE),
        Tier::Secondary | Tier::Chip { .. } => theme::medium(TEXT_SIZE),
    }
}

/// The green capsule for the one action a view leads with: Create, Save,
/// Sign in, Apply.
pub fn primary(ui: &mut Ui, palette: &Palette, label: &str) -> Response {
    text_button(
        ui,
        palette,
        Tier::Primary,
        None,
        label,
        tokens::button::PRIMARY,
    )
    .0
}

/// A taller [`primary`] for the sign-in screen, where it is the only
/// control: as wide as the column, up to 300 points.
pub fn primary_large(ui: &mut Ui, palette: &Palette, label: &str) -> Response {
    let width = ui.available_width().min(300.0);
    text_button_inner(
        ui,
        palette,
        Tier::Primary,
        None,
        label,
        Vec2::new(width, tokens::button::PRIMARY_LARGE),
        false,
    )
    .0
}

/// A grey capsule for every other labelled action: Cancel, Retry, Follow,
/// Load more. `icon` leads the label.
pub fn secondary(ui: &mut Ui, palette: &Palette, icon: Option<Icon>, label: &str) -> Response {
    text_button(
        ui,
        palette,
        Tier::Secondary,
        icon,
        label,
        tokens::button::SECONDARY,
    )
    .0
}

/// One choice among several, such as a filter or a bitrate: inverted while
/// it is the chosen one.
pub fn chip(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    selected: bool,
) -> Response {
    let response = text_button(
        ui,
        palette,
        Tier::Chip { selected },
        icon,
        label,
        tokens::button::SECONDARY,
    )
    .0;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    response
}

/// A [`chip`] whose icon turns into a cross on hover, so the entry can be
/// dismissed. The flag reports clicks on that cross.
pub fn chip_dismissible(
    ui: &mut Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
) -> (Response, bool) {
    text_button_inner(
        ui,
        palette,
        Tier::Chip { selected: false },
        Some(icon),
        label,
        Vec2::new(0.0, tokens::button::SECONDARY),
        true,
    )
}

/// The width a [`secondary`] button or a [`chip`] without an icon takes,
/// for laying out a row of them before drawing it.
pub fn text_button_width(ui: &Ui, label: &str) -> f32 {
    let tier = Tier::Secondary;
    let galley = crate::bidi::layout_line(ui.painter(), label, font(tier), Color32::WHITE);
    galley.size().x + padding(tier) * 2.0
}

fn text_button(
    ui: &mut Ui,
    palette: &Palette,
    tier: Tier,
    icon: Option<Icon>,
    label: &str,
    height: f32,
) -> (Response, bool) {
    text_button_inner(
        ui,
        palette,
        tier,
        icon,
        label,
        Vec2::new(0.0, height),
        false,
    )
}

fn text_button_inner(
    ui: &mut Ui,
    palette: &Palette,
    tier: Tier,
    icon: Option<Icon>,
    label: &str,
    least: Vec2,
    dismissible: bool,
) -> (Response, bool) {
    let height = least.y;
    let color = match tier {
        Tier::Primary => palette.on_accent,
        Tier::Chip { selected: true } => palette.window,
        Tier::Secondary | Tier::Chip { selected: false } => palette.text,
    };
    let galley = crate::bidi::layout_line(ui.painter(), label, font(tier), color);
    let pad = padding(tier);
    let icon_width = if icon.is_some() {
        TEXT_ICON + TEXT_ICON_GAP
    } else {
        0.0
    };
    let size = Vec2::new(
        (galley.size().x + icon_width + pad * 2.0).max(least.x),
        height,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let content = galley.size().x + icon_width;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(
            rect.center().x - content / 2.0 + TEXT_ICON / 2.0,
            rect.center().y,
        ),
        Vec2::splat(TEXT_ICON),
    );
    // Claimed after the button, so the cross sits on top and keeps its own click.
    let dismiss = (dismissible && icon.is_some()).then(|| {
        let dismiss = ui.interact(
            icon_rect.expand(4.0),
            response.id.with("dismiss"),
            Sense::click(),
        );
        dismiss.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                format!("Remove {label}"),
            )
        });
        dismiss
    });
    let dismissed = dismiss.as_ref().is_some_and(Response::clicked);
    let over_dismiss = dismiss
        .as_ref()
        .is_some_and(|dismiss| dismiss.hovered() || dismiss.has_focus());
    let corner = tokens::capsule(height);
    let enabled = ui.is_enabled();
    let hovered = enabled && (response.hovered() || over_dismiss);
    let feedback = Feedback::of(ui, &response, hovered);
    if ui.is_rect_visible(rect) {
        let fill = match tier {
            Tier::Primary => feedback.fill(
                palette.accent,
                palette.accent_hover,
                palette.accent.gamma_multiply(0.92),
            ),
            Tier::Chip { selected: true } => palette.text,
            // The grey fills follow the Ui, so a card can lift them: see
            // `on_card`. Outside a card they are the palette's surfaces.
            _ => {
                let widgets = &ui.visuals().widgets;
                feedback.fill(
                    widgets.inactive.weak_bg_fill,
                    widgets.hovered.weak_bg_fill,
                    widgets.active.weak_bg_fill,
                )
            }
        };
        let fill = if enabled {
            fill
        } else {
            fill.gamma_multiply(0.4)
        };
        let color = if enabled {
            color
        } else {
            color.gamma_multiply(0.4)
        };
        ui.painter().rect_filled(rect, corner, fill);
        // Content is centred when a least width leaves room around it.
        let mut x = rect.center().x - content / 2.0;
        if let Some(icon) = icon {
            let icon = if dismiss.is_some() && hovered {
                Icon::X
            } else {
                icon
            };
            icon.image(color, TEXT_ICON).paint_at(ui, icon_rect);
            x += icon_width;
        }
        let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, color);
    }
    focus_ring(ui, &response, corner);
    if let Some(dismiss) = dismiss {
        focus_ring(ui, &dismiss, CornerRadius::same(tokens::radius::ROUND));
    }
    (response, dismissed)
}

/// Makes the grey text buttons and chips drawn in `ui` stand out from a
/// card filled with `surface`, such as the artist card or a Settings
/// section: they rest on [`Palette::on_card`] instead of the card's own
/// grey. Call it once at the top of the card's contents; it lasts for
/// that Ui and everything inside it.
pub fn on_card(ui: &mut Ui, palette: &Palette) {
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill = palette.on_card();
    widgets.hovered.weak_bg_fill = palette.on_card_hover();
    widgets.active.weak_bg_fill = palette.surface_hover;
}

/// The green Play disc's three sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscSize {
    /// A page header: album, playlist, artist, show.
    Large,
    /// Cards, tiles, the top search result and the library grid.
    Medium,
    /// Rows, episodes and covers in a list.
    Small,
}

impl DiscSize {
    pub const fn diameter(self) -> f32 {
        match self {
            Self::Large => tokens::disc::LARGE,
            Self::Medium => tokens::disc::MEDIUM,
            Self::Small => tokens::disc::SMALL,
        }
    }
}

/// The icon inside a disc, as a share of its diameter.
const DISC_ICON: f32 = 0.42;

/// The one control that starts playback of a context: a green disc with
/// Play, or Pause while that context plays. The player bar's own Play and
/// Pause is [`PlayDisc::transport`], a neutral disc.
#[must_use = "a play disc does nothing until shown"]
pub struct PlayDisc<'a> {
    diameter: f32,
    playing: bool,
    label: &'a str,
    on_art: bool,
    transport: bool,
}

impl<'a> PlayDisc<'a> {
    pub fn new(size: DiscSize, label: &'a str) -> Self {
        Self {
            diameter: size.diameter(),
            playing: false,
            label,
            on_art: false,
            transport: false,
        }
    }

    /// The player bar's neutral Play and Pause.
    pub fn transport(label: &'a str) -> Self {
        Self {
            diameter: tokens::disc::TRANSPORT,
            transport: true,
            ..Self::new(DiscSize::Medium, label)
        }
    }

    /// Shows Pause instead of Play.
    pub fn playing(mut self, playing: bool) -> Self {
        self.playing = playing;
        self
    }

    /// Lifts the disc off cover art with a soft shadow.
    pub fn on_art(mut self, on_art: bool) -> Self {
        self.on_art = on_art;
        self
    }

    pub fn show(self, ui: &mut Ui, palette: &Palette) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(self.diameter), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), self.label)
        });
        let feedback = Feedback::of(ui, &response, response.hovered() || response.has_focus());
        if ui.is_rect_visible(rect) {
            self.paint_feedback(ui, palette, rect.center(), feedback);
        }
        focus_ring(ui, &response, CornerRadius::same(tokens::radius::ROUND));
        response.on_hover_text(self.label)
    }

    /// Paints the disc at `center` for a control that handles its own
    /// clicks, such as a row whose cover plays it.
    pub fn paint(
        &self,
        ui: &Ui,
        palette: &Palette,
        center: egui::Pos2,
        hovered: bool,
        pressed: bool,
    ) {
        let amount = |on: bool| if on { 1.0 } else { 0.0 };
        let feedback = Feedback {
            hover: amount(hovered),
            press: amount(pressed),
        };
        self.paint_feedback(ui, palette, center, feedback);
    }

    /// Paints the disc with hover and press part of the way in.
    pub fn paint_feedback(
        &self,
        ui: &Ui,
        palette: &Palette,
        center: egui::Pos2,
        feedback: Feedback,
    ) {
        let (fill, fill_hover, icon_color) = if self.transport {
            let hover = if palette.dark {
                Color32::WHITE
            } else {
                palette.text
            };
            (palette.text, hover, palette.window)
        } else {
            (palette.accent, palette.accent_hover, palette.on_accent)
        };
        let scale = 1.0 - 0.04 * feedback.press;
        let radius = self.diameter / 2.0 * scale;
        if self.on_art {
            ui.painter().add(
                egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 12,
                    spread: 0,
                    color: Color32::from_black_alpha(90),
                }
                .as_shape(
                    egui::Rect::from_center_size(center, Vec2::splat(radius * 2.0)),
                    CornerRadius::same(tokens::radius::ROUND),
                ),
            );
        }
        let fill = motion::mix(fill, fill_hover, feedback.hover);
        ui.painter().circle_filled(center, radius, fill);
        let icon = if self.playing {
            Icon::PauseFilled
        } else {
            Icon::PlayFilled
        };
        let size = self.diameter * DISC_ICON * scale;
        let icon_rect = egui::Rect::from_center_size(
            center + theme::play_glyph_offset(icon, size),
            Vec2::splat(size),
        );
        icon.image(icon_color, size).paint_at(ui, icon_rect);
    }

    /// Where the disc sits over the bottom-right corner of `art`, `inset`
    /// in from its edges.
    pub fn corner_rect(&self, art: egui::Rect, inset: f32) -> egui::Rect {
        egui::Rect::from_center_size(
            art.right_bottom() - Vec2::splat(inset + self.diameter / 2.0),
            Vec2::splat(self.diameter),
        )
    }
}

/// The row under a page's header: album, playlist, radio, artist and
/// podcast. It leads with the large [`PlayDisc`], then large icon buttons
/// (or Follow, for an artist) and More last, all spaced alike, and keeps
/// the same gap before the page's list.
pub fn action_row<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = tokens::gap::ACTIONS;
            ui.set_min_height(tokens::disc::LARGE);
            contents(ui)
        })
        .inner;
    ui.add_space(tokens::gap::BELOW_ACTIONS);
    inner
}

/// A [`PlayDisc`] whose icon is replaced by a spinner while playback
/// starts: the pressed disc itself shows that Spotify is reacting.
pub fn play_disc_spinner(ui: &mut Ui, palette: &Palette, size: DiscSize, label: &str) -> Response {
    let diameter = size.diameter();
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, ui.is_enabled(), label)
    });
    if ui.is_rect_visible(rect) {
        ui.painter()
            .circle_filled(rect.center(), diameter / 2.0, palette.accent);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        ));
        theme::spinner(&mut child, diameter * 0.55, palette.on_accent);
    }
    response.on_hover_text(label)
}

/// Draws a [`PlayDisc`] over the bottom-right corner of cover art, the
/// way cards, tiles and the library grid reveal Play on hover. `inset` is
/// the gap between the disc and the art's edges, and `reveal` how far it
/// has faded in, from [`motion::hover`] on whatever reveals it. Nothing is
/// drawn or clickable while it is hidden.
pub fn hover_play(
    ui: &mut Ui,
    palette: &Palette,
    art: egui::Rect,
    inset: f32,
    reveal: f32,
    disc: PlayDisc<'_>,
) -> Option<Response> {
    if reveal <= 0.0 {
        return None;
    }
    let rect = disc.corner_rect(art, inset);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
    ));
    child.multiply_opacity(reveal);
    Some(disc.on_art(true).show(&mut child, palette))
}
