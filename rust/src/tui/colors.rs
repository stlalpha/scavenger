//! Scavenger's dark palette.
//!
//! Orange is the accent — it marks focus, price, and interactive hints
//! everywhere (see the splitter drag color). Text uses a single warm-gray
//! ramp from bright primary down to disabled, so hierarchy reads by
//! brightness alone rather than hue. Panel backgrounds differ by a couple
//! of RGB steps: enough to separate sidebar/feed/detail at a glance,
//! subtle enough to stay out of the way.

use ratatui::style::Color;

// -- Accents --
pub const ORANGE: Color = Color::Rgb(0xfd, 0x97, 0x1f);
pub const GREEN: Color = Color::Rgb(0xa6, 0xe2, 0x2e);
pub const BLUE: Color = Color::Rgb(0x66, 0xd9, 0xef);
pub const PINK: Color = Color::Rgb(0xf9, 0x26, 0x72);
pub const YELLOW: Color = Color::Rgb(0xe6, 0xdb, 0x74);

// -- Text ramp, brightest to dimmest --
pub const TEXT_PRIMARY: Color = Color::Rgb(0xf4, 0xf4, 0xf1);
pub const TEXT_MUTED: Color = Color::Rgb(0xab, 0xa8, 0x96);
pub const TEXT_COMMENT: Color = Color::Rgb(0x8f, 0x8b, 0x76);
pub const TEXT_DIM: Color = Color::Rgb(0x74, 0x70, 0x5f);
pub const TEXT_DIMMER: Color = Color::Rgb(0x5c, 0x58, 0x4a);
pub const TEXT_DIMMEST: Color = Color::Rgb(0x45, 0x41, 0x35);
pub const TEXT_DISABLED: Color = Color::Rgb(0x38, 0x35, 0x2b);

// -- Panel backgrounds, subtly differentiated --
pub const BG_SIDEBAR: Color = Color::Rgb(0x16, 0x15, 0x12);
pub const BG_FEED: Color = Color::Rgb(0x13, 0x13, 0x11);
pub const BG_DETAIL: Color = Color::Rgb(0x11, 0x10, 0x0e);

// -- Selection / highlight washes, orange-tinted to match the accent --
pub const BG_SELECTED: Color = Color::Rgb(0x2e, 0x26, 0x18);
pub const BG_HIGHLIGHT: Color = Color::Rgb(0x26, 0x20, 0x14);

/// Idle border / indicator color — used for unfocused panel borders and
/// the default (non-unread) profile arrow.
pub const INDICATOR_ACTIVE: Color = Color::Rgb(0x46, 0x44, 0x3a);

// -- Chrome: status bar, log panel, splitters. Backgrounds distinct enough
// from the panel ramp above that they read as UI chrome rather than content. --
pub const BG_STATUS_BAR: Color = Color::Rgb(0x11, 0x11, 0x11);
pub const BG_LOG: Color = Color::Rgb(0x18, 0x18, 0x18);
pub const TEXT_DARK: Color = Color::Rgb(0x3a, 0x3a, 0x3a);
pub const BOT_BLOCK_BG: Color = Color::Rgb(0x3a, 0x1a, 0x1a);
pub const SPLITTER_IDLE: Color = Color::Rgb(0x22, 0x22, 0x22);
pub const SPLITTER_HOVER: Color = Color::Rgb(0x55, 0x55, 0x55);

/// Per-source accent color, used for badges and detail-panel labels.
pub fn src_color(source: &str) -> Color {
    match source {
        "ebay" => YELLOW,
        "craigslist" => PINK,
        "facebook" => BLUE,
        _ => TEXT_MUTED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn src_color_known_sources() {
        assert_eq!(src_color("ebay"), YELLOW);
        assert_eq!(src_color("craigslist"), PINK);
        assert_eq!(src_color("facebook"), BLUE);
    }

    #[test]
    fn src_color_unknown_falls_back_to_muted() {
        assert_eq!(src_color("mystery"), TEXT_MUTED);
    }
}
