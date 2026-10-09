//! Design tokens: the sizes, corner radii and timings the interface is
//! built from, named once so every surface agrees. Draw with these instead
//! of numbers; a new size or radius belongs here first.

use egui::CornerRadius;

/// Corner radii, from thin bars to the sign-in card. Capsules and discs
/// round by half their height instead, through [`capsule`].
pub mod radius {
    /// Progress and volume bars, the explicit badge.
    pub const BAR: u8 = 2;
    /// Covers in song rows.
    pub const COVER_SMALL: u8 = 4;
    /// Row and menu item highlights, library covers, tiles.
    pub const ROW: u8 = 6;
    /// Cards, menus, toasts and egui's own popups.
    pub const CARD: u8 = 8;
    /// Cards inside a panel, and windows.
    pub const PANEL: u8 = 10;
    /// Dialogs and the update window.
    pub const DIALOG: u8 = 12;
    /// The sign-in card.
    pub const SHEET: u8 = 16;
    /// As round as egui draws: a circle for any square up to 254 points.
    pub const ROUND: u8 = 127;
}

/// A capsule's corners: fully round ends for a control `height` tall.
pub fn capsule(height: f32) -> CornerRadius {
    CornerRadius::same((height / 2.0).round().clamp(0.0, f32::from(radius::ROUND)) as u8)
}

/// A radius as the points egui's painter takes.
pub fn points(radius: u8) -> f32 {
    f32::from(radius)
}

/// Icon sizes. Lucide's 2-point stroke on its 24 grid stays between 1.3
/// and 2 points only at these sizes.
pub mod icon {
    pub const SMALL: f32 = 16.0;
    pub const MEDIUM: f32 = 20.0;
    pub const LARGE: f32 = 24.0;
}

/// The square an icon button answers to around each icon size.
pub mod hit {
    /// Rows, menus and the dense corners of panels; a 16-point icon.
    pub const COMPACT: f32 = 28.0;
    /// Toolbars and panel headers; a 20-point icon.
    pub const STANDARD: f32 = 32.0;
    /// The action row under a page's header; a 24-point icon.
    pub const LARGE: f32 = 40.0;
    /// A disc laid over cover art, such as the player bar's expand
    /// chevron; a 16-point icon on a backing that keeps it legible.
    pub const OVERLAY: f32 = 24.0;
}

/// The green Play disc's diameters, and the player bar's own transport disc.
pub mod disc {
    /// A page header: album, playlist, artist, show.
    pub const LARGE: f32 = 56.0;
    /// Cards, tiles, the top search result and the library grid.
    pub const MEDIUM: f32 = 44.0;
    /// Rows, episodes and small overlays.
    pub const SMALL: f32 = 32.0;
    /// The player bar's neutral Play and Pause.
    pub const TRANSPORT: f32 = 36.0;
}

/// Text button heights.
pub mod button {
    /// The one green action a view leads with.
    pub const PRIMARY: f32 = 36.0;
    /// The sign-in screen's one action.
    pub const PRIMARY_LARGE: f32 = 44.0;
    /// Every other labelled action, and segmented chips.
    pub const SECONDARY: f32 = 32.0;
}

/// A side panel's header.
pub mod panel {
    /// The title's size, the same in every panel.
    pub const TITLE: f32 = 16.0;
    /// How far the title and tabs sit in from the panel's edge.
    pub const INSET: f32 = 4.0;
    /// Between the title row and a line of tabs under it.
    pub const TABS_GAP: f32 = 6.0;
    /// Between the header and the panel's contents.
    pub const BELOW: f32 = 8.0;
}

/// Spacing between controls.
pub mod gap {
    /// Between icon buttons in one group.
    pub const ICONS: f32 = 4.0;
    /// Between groups of controls.
    pub const GROUP: f32 = 12.0;
}

/// Durations in seconds. Everything that moves eases through egui's
/// `animate_*_with_time`, which stops asking for frames once it settles.
pub mod motion {
    /// Hover and press feedback, row reveals, toggled dots.
    pub const FAST: f32 = 0.12;
    /// Menus and popovers opening.
    pub const BASE: f32 = 0.18;
    /// Menus and popovers closing.
    pub const BASE_OUT: f32 = 0.12;
    /// Side panels and the sidebar showing or hiding.
    pub const PANEL: f32 = 0.22;
    /// A new page fading in.
    pub const PAGE: f32 = 0.2;
    /// Content changing in place: the player bar's song, dialogs.
    pub const CONTENT: f32 = 0.25;
    /// Rows joining or leaving a list.
    pub const LIST: f32 = 0.16;
}

// Closing is never slower than opening, and feedback is quicker than
// the panels it opens.
const _: () = assert!(motion::BASE_OUT <= motion::BASE && motion::FAST < motion::PANEL);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capsule_rounds_by_half_its_height() {
        assert_eq!(capsule(32.0), CornerRadius::same(16));
        assert_eq!(capsule(27.5), CornerRadius::same(14));
        assert_eq!(capsule(1000.0), CornerRadius::same(radius::ROUND));
        assert_eq!(capsule(-4.0), CornerRadius::same(0));
    }

    #[test]
    fn each_icon_size_has_its_hit_target() {
        // The padding around an icon grows with it: 6, 6 and 8 points.
        assert_eq!(hit::COMPACT - icon::SMALL, 12.0);
        assert_eq!(hit::STANDARD - icon::MEDIUM, 12.0);
        assert_eq!(hit::LARGE - icon::LARGE, 16.0);
    }

    #[test]
    fn radii_grow_with_the_surface() {
        let radii = [
            radius::BAR,
            radius::COVER_SMALL,
            radius::ROW,
            radius::CARD,
            radius::PANEL,
            radius::DIALOG,
            radius::SHEET,
        ];
        assert!(radii.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
