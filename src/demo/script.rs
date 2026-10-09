//! The scripted states a demo capture describes on the command line: its
//! size, and a click or a drag held through the same code a real pointer
//! runs. Shared by `--demo-shot` (a window) and the headless `render`
//! example (no window).

use crate::app::App;
use crate::i18n::Locale;

/// `WIDTHxHEIGHT` in points, as `--demo-size` takes it.
pub fn parse_size(spec: &str) -> Result<[f32; 2], String> {
    let (width, height) = spec
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected WIDTHxHEIGHT, got {spec}"))?;
    let width: f32 = width
        .trim()
        .parse()
        .map_err(|_| format!("invalid width in {spec}"))?;
    let height: f32 = height
        .trim()
        .parse()
        .map_err(|_| format!("invalid height in {spec}"))?;
    if width < 1.0 || height < 1.0 {
        return Err(format!("size must be at least 1x1, got {spec}"));
    }
    Ok([width, height])
}

/// `X,Y` in points, as `--demo-click` takes it.
pub fn parse_point(point: &str) -> Result<egui::Pos2, String> {
    let (x, y) = point
        .split_once(',')
        .ok_or_else(|| format!("expected X,Y, got {point}"))?;
    let x: f32 = x
        .trim()
        .parse()
        .map_err(|_| format!("invalid x in {point}"))?;
    let y: f32 = y
        .trim()
        .parse()
        .map_err(|_| format!("invalid y in {point}"))?;
    Ok(egui::pos2(x, y))
}

/// `X,Y:X,Y` in points, as `--demo-drag` takes it.
pub fn parse_drag(spec: &str) -> Result<[egui::Pos2; 2], String> {
    let (from, to) = spec
        .split_once(':')
        .ok_or_else(|| format!("expected X,Y:X,Y, got {spec}"))?;
    Ok([parse_point(from)?, parse_point(to)?])
}

/// Shows the interface in `locale`, as `--demo-language` does.
pub fn set_language(app: &mut App, locale: Locale) {
    app.settings.language = crate::settings::LanguageChoice::Locale(locale);
    app.locale = locale;
}

/// A scripted pointer for `--demo-drag`: rest on `from`, press there, glide
/// to `to` and hold, so the shot shows a drag in progress through the same
/// code a real pointer runs. For `--demo-click` it releases right after
/// the press instead, where it pressed.
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    from: egui::Pos2,
    to: egui::Pos2,
    frame: u32,
    release: bool,
}

impl Pointer {
    /// Frames to rest before pressing, so the rows under `from` are laid out.
    pub const REST: u32 = 20;
    /// Frames the glide from `from` to `to` takes.
    pub const GLIDE: u32 = 20;

    /// Press at `from`, glide to `to`, and hold there.
    pub fn drag(from: egui::Pos2, to: egui::Pos2) -> Self {
        Self {
            from,
            to,
            frame: 0,
            release: false,
        }
    }

    /// Press and release once at `at`.
    pub fn click(at: egui::Pos2) -> Self {
        Self {
            from: at,
            to: at,
            frame: 0,
            release: true,
        }
    }

    /// The scripted pointer, from `--demo-drag` or else `--demo-click`.
    pub fn from_flags(drag: Option<[egui::Pos2; 2]>, click: Option<egui::Pos2>) -> Option<Self> {
        drag.map(|[from, to]| Self::drag(from, to))
            .or(click.map(Self::click))
    }

    /// Whether the script has nothing left to do: the press, the glide and
    /// any release are behind it.
    pub fn done(&self) -> bool {
        self.frame > Self::REST + Self::GLIDE
    }

    /// This frame's pointer events.
    pub fn events(&mut self) -> Vec<egui::Event> {
        let frame = self.frame;
        self.frame = self.frame.saturating_add(1);
        let pos = if frame <= Self::REST {
            self.from
        } else {
            let t = ((frame - Self::REST) as f32 / Self::GLIDE as f32).min(1.0);
            self.from.lerp(self.to, t)
        };
        let mut events = vec![egui::Event::PointerMoved(pos)];
        if frame == Self::REST {
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            });
        }
        if self.release && frame == Self::REST + 1 {
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_parses_width_by_height() {
        assert_eq!(parse_size("760x800").unwrap(), [760.0, 800.0]);
        assert_eq!(parse_size("1280X800").unwrap(), [1280.0, 800.0]);
        assert!(parse_size("wide").is_err());
        assert!(parse_size("0x800").is_err());
    }

    #[test]
    fn drag_presses_at_the_first_point_and_holds_at_the_second() {
        let [from, to] = parse_drag("10,20:110, 220").unwrap();
        assert_eq!(
            (from, to),
            (egui::pos2(10.0, 20.0), egui::pos2(110.0, 220.0))
        );
        assert!(parse_drag("10,20").is_err());
        assert!(parse_drag("10:20").is_err());
        let mut drag = Pointer::drag(from, to);
        let frames: Vec<_> = (0..=Pointer::REST + Pointer::GLIDE + 5)
            .map(|_| drag.events())
            .collect();
        let presses = frames
            .iter()
            .flatten()
            .filter(|event| matches!(event, egui::Event::PointerButton { pressed: true, .. }))
            .count();
        assert_eq!(presses, 1, "one press, never a release");
        assert!(
            !frames
                .iter()
                .flatten()
                .any(|event| matches!(event, egui::Event::PointerButton { pressed: false, .. }))
        );
        assert_eq!(frames[0][0], egui::Event::PointerMoved(from));
        assert_eq!(frames.last().unwrap()[0], egui::Event::PointerMoved(to));
        assert!(drag.done());
    }

    #[test]
    fn click_presses_and_releases_once_in_place() {
        let at = parse_point("40, 30").unwrap();
        assert_eq!(at, egui::pos2(40.0, 30.0));
        let mut click = Pointer::from_flags(None, Some(at)).unwrap();
        assert!(!click.done());
        let buttons: Vec<_> = (0..=Pointer::REST + Pointer::GLIDE + 5)
            .flat_map(|_| click.events())
            .filter_map(|event| match event {
                egui::Event::PointerButton { pos, pressed, .. } => Some((pos, pressed)),
                _ => None,
            })
            .collect();
        assert_eq!(buttons, [(at, true), (at, false)]);
    }
}
