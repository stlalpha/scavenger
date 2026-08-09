use std::path::PathBuf;
use std::process::Command;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Widget, Wrap},
};

use crate::models::{Listing, ListingStatus};
use crate::tui::colors;

fn status_span(status: &ListingStatus) -> Span<'static> {
    match status {
        ListingStatus::New => Span::styled(
            "NEW",
            Style::default().fg(colors::BLUE).add_modifier(Modifier::BOLD),
        ),
        ListingStatus::Seen => Span::styled("SEEN", Style::default().fg(colors::TEXT_MUTED)),
        ListingStatus::Saved => Span::styled("SAVED", Style::default().fg(colors::GREEN)),
        ListingStatus::Dismissed => Span::styled("DISMISSED", Style::default().fg(colors::TEXT_MUTED)),
        ListingStatus::Snoozed => Span::styled("SNOOZED", Style::default().fg(colors::YELLOW)),
    }
}

fn render_listing_text(listing: &Listing, img_index: usize, img_total: usize) -> Text<'static> {
    let mut lines: Vec<Line> = Vec::new();

    // Title
    lines.push(Line::from(Span::styled(
        listing.title.clone(),
        Style::default()
            .fg(colors::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::raw(""));

    // Price + source + status
    let price = match listing.price {
        Some(p) => Span::styled(
            format!("${:.2}", p),
            Style::default()
                .fg(colors::ORANGE)
                .add_modifier(Modifier::BOLD),
        ),
        None => Span::styled("no price", Style::default().fg(colors::TEXT_DIMMER)),
    };

    let src = Span::styled(
        listing.source_id.to_uppercase(),
        Style::default()
            .fg(colors::src_color(&listing.source_id))
            .add_modifier(Modifier::BOLD),
    );

    lines.push(Line::from(vec![
        price,
        Span::raw("  "),
        src,
        Span::raw("  "),
        status_span(&listing.status),
    ]));

    // Location
    if let Some(ref loc) = listing.location {
        lines.push(Line::from(Span::styled(
            loc.clone(),
            Style::default().fg(colors::TEXT_MUTED),
        )));
    }

    lines.push(Line::raw(""));

    // URL
    lines.push(Line::from(Span::styled(
        listing.url.clone(),
        Style::default()
            .fg(colors::TEXT_DIMMER)
            .add_modifier(Modifier::UNDERLINED),
    )));

    // AI evaluation
    if let Some(ref eval_json) = listing.ai_evaluation {
        if let Ok(ev) = serde_json::from_str::<serde_json::Value>(eval_json) {
            let notable = ev.get("notable").and_then(|v| v.as_str()).unwrap_or("");
            let reason = ev.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            if !notable.is_empty() || !reason.is_empty() {
                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled(
                    "AI INSIGHT",
                    Style::default()
                        .fg(colors::PINK)
                        .add_modifier(Modifier::BOLD),
                )));
                if !notable.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "!",
                            Style::default()
                                .fg(colors::PINK)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(notable.to_string(), Style::default().fg(colors::TEXT_PRIMARY)),
                    ]));
                }
                if !reason.is_empty() {
                    lines.push(Line::from(Span::styled(
                        reason.to_string(),
                        Style::default().fg(colors::TEXT_COMMENT),
                    )));
                }
            }
        }
    }

    // Description
    if !listing.description.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            listing.description.clone(),
            Style::default().fg(colors::TEXT_COMMENT),
        )));
    }

    lines.push(Line::raw(""));

    // Image nav + keybind hints. Labeled with the actual keys (',' / '.')
    // rather than plain chevrons — arrow keys switch panel focus instead.
    let mut hint_spans: Vec<Span> = Vec::new();
    if img_total > 1 {
        hint_spans.push(Span::styled(",", Style::default().fg(colors::ORANGE)));
        hint_spans.push(Span::raw(" "));
        hint_spans.push(Span::styled(
            format!("{}/{}", img_index + 1, img_total),
            Style::default().fg(colors::TEXT_PRIMARY),
        ));
        hint_spans.push(Span::raw(" "));
        hint_spans.push(Span::styled(".", Style::default().fg(colors::ORANGE)));
        hint_spans.push(Span::raw("  "));
    } else if img_total == 1 {
        hint_spans.push(Span::styled("1/1", Style::default().fg(colors::TEXT_DIMMEST)));
        hint_spans.push(Span::raw("  "));
    }

    let keybinds: &[(&str, &str)] = &[("o", "pen"), ("s", "ave"), ("d", "ism"), ("n", "ap")];
    for (i, &(key, suffix)) in keybinds.iter().enumerate() {
        if i > 0 {
            hint_spans.push(Span::raw("  "));
        }
        hint_spans.push(Span::styled(
            key.to_string(),
            Style::default().fg(colors::ORANGE),
        ));
        hint_spans.push(Span::styled(
            suffix.to_string(),
            Style::default().fg(colors::TEXT_MUTED),
        ));
    }

    lines.push(Line::from(hint_spans));

    Text::from(lines)
}

/// Detail panel showing the selected listing's full information.
pub struct DetailPanel {
    listing: Option<Listing>,
    images: Vec<String>,
    img_idx: usize,
    /// Resolved path for the current hero image (if any).
    pub hero_image_path: Option<PathBuf>,
}

impl Default for DetailPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl DetailPanel {
    pub fn new() -> Self {
        Self {
            listing: None,
            images: Vec::new(),
            img_idx: 0,
            hero_image_path: None,
        }
    }

    pub fn current_listing(&self) -> Option<&Listing> {
        self.listing.as_ref()
    }

    pub fn has_ai_notes(&self) -> bool {
        self.listing
            .as_ref()
            .and_then(|l| l.ai_evaluation.as_deref())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .map(|v| {
                v.get("reason")
                    .and_then(|r| r.as_str())
                    .is_some_and(|s| !s.is_empty())
                    || v.get("notable")
                        .and_then(|n| n.as_str())
                        .is_some_and(|s| !s.is_empty())
            })
            .unwrap_or(false)
    }

    /// Show a new listing. Returns true if the listing changed.
    pub fn show_listing(&mut self, listing: Option<Listing>) -> bool {
        if let (Some(new), Some(old)) = (&listing, &self.listing) {
            if new.id == old.id {
                return false;
            }
        }
        self.listing = listing;
        self.images = self
            .listing
            .as_ref()
            .map(|l| l.image_urls.clone())
            .unwrap_or_default();
        self.img_idx = 0;
        self.hero_image_path = None;
        true
    }

    /// Set full gallery images (from detail page scrape), replacing the
    /// feed-thumbnail placeholder set by `show_listing`. Deliberately does
    /// NOT touch `hero_image_path` — the gallery's URLs are a different
    /// size variant than the feed thumbnail, so swapping eagerly here would
    /// blank the hero every time while the new size downloads. The caller
    /// keeps showing the current image until the replacement actually
    /// resolves (cache hit or a completed download).
    pub fn set_gallery(&mut self, images: Vec<String>) {
        if images.is_empty() {
            return;
        }
        let current_url = self.images.get(self.img_idx).cloned();
        self.images = images;
        self.img_idx = current_url
            .and_then(|u| self.images.iter().position(|i| *i == u))
            .unwrap_or(0);
    }

    pub fn current_image_url(&self) -> Option<&str> {
        self.images.get(self.img_idx).map(|s| s.as_str())
    }

    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    pub fn image_index(&self) -> usize {
        self.img_idx
    }

    pub fn next_image(&mut self) -> bool {
        if self.images.len() > 1 {
            self.img_idx = (self.img_idx + 1) % self.images.len();
            self.hero_image_path = None;
            true
        } else {
            false
        }
    }

    pub fn prev_image(&mut self) -> bool {
        if self.images.len() > 1 {
            self.img_idx = (self.img_idx + self.images.len() - 1) % self.images.len();
            self.hero_image_path = None;
            true
        } else {
            false
        }
    }

    pub fn open_in_browser(&self) {
        let url = match &self.listing {
            Some(l) => &l.url,
            None => return,
        };
        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("open")
                .args(["-na", "Google Chrome", "--args", "--profile-directory=Default", url])
                .spawn();
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = Command::new("xdg-open").arg(url).spawn();
        }
    }

    /// Renders the panel and, if a hero image is loaded, returns the Rect
    /// reserved for it — painting the image itself (via ratatui_image) is
    /// the caller's job, since that needs a live `Picker` the widget layer
    /// doesn't own.
    pub fn render(&self, area: Rect, buf: &mut Buffer, focused: bool) -> Option<Rect> {
        let border_color = if focused {
            colors::ORANGE
        } else {
            colors::INDICATOR_ACTIVE
        };

        let src_label = self
            .listing
            .as_ref()
            .map(|l| {
                let clr = colors::src_color(&l.source_id);
                vec![
                    Span::raw(" "),
                    Span::styled(
                        l.source_id.to_uppercase(),
                        Style::default().fg(clr),
                    ),
                ]
            })
            .unwrap_or_default();

        let mut title_spans = vec![Span::styled(
            " DETAIL",
            Style::default()
                .fg(colors::ORANGE)
                .add_modifier(Modifier::BOLD),
        )];
        title_spans.extend(src_label);

        let block = Block::bordered()
            .title(Line::from(title_spans))
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(colors::BG_DETAIL));

        let inner = block.inner(area);
        Widget::render(block, area, buf);

        if inner.width == 0 || inner.height == 0 {
            return None;
        }

        match &self.listing {
            None => {
                let placeholder = Paragraph::new(Span::styled(
                    "  select a listing",
                    Style::default()
                        .fg(colors::TEXT_MUTED)
                        .add_modifier(Modifier::ITALIC),
                ));
                Widget::render(placeholder, inner, buf);
                None
            }
            Some(listing) => {
                // Split inner area: hero image area (if we have an image path) + text.
                // The band is capped well below inner.height so text always
                // keeps room to render; below 10 rows there isn't enough
                // space for both, so skip the image entirely.
                let (text_area, image_area) = if self.hero_image_path.is_some()
                    && inner.height >= 10
                {
                    let image_height = (inner.height / 2)
                        .clamp(4, 16)
                        .min(inner.height.saturating_sub(6));
                    let chunks = Layout::vertical([
                        Constraint::Length(image_height),
                        Constraint::Min(0),
                    ])
                    .split(inner);
                    (chunks[1], Some(chunks[0]))
                } else {
                    (inner, None)
                };

                let text = render_listing_text(listing, self.img_idx, self.images.len());
                let content = Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .style(Style::default().bg(colors::BG_DETAIL));
                Widget::render(content, text_area, buf);

                image_area
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ListingStatus;
    use chrono::Utc;

    fn listing_with(id: &str, images: Vec<&str>, ai_evaluation: Option<&str>) -> Listing {
        Listing {
            id: id.to_string(),
            profile_id: "p1".to_string(),
            source_id: "ebay".to_string(),
            title: "widget".to_string(),
            description: String::new(),
            price: None,
            currency: "USD".to_string(),
            condition: None,
            url: "https://example.com/a".to_string(),
            image_urls: images.into_iter().map(String::from).collect(),
            location: None,
            first_seen: Utc::now(),
            last_seen: Utc::now(),
            relevance_score: 0.0,
            status: ListingStatus::New,
            ai_evaluation: ai_evaluation.map(String::from),
        }
    }

    #[test]
    fn show_listing_reports_change_only_on_new_id() {
        let mut panel = DetailPanel::new();
        assert!(panel.show_listing(Some(listing_with("a", vec![], None))));
        assert!(!panel.show_listing(Some(listing_with("a", vec![], None))));
        assert!(panel.show_listing(Some(listing_with("b", vec![], None))));
    }

    #[test]
    fn image_navigation_wraps() {
        let mut panel = DetailPanel::new();
        panel.show_listing(Some(listing_with("a", vec!["a.jpg", "b.jpg", "c.jpg"], None)));
        assert_eq!(panel.image_index(), 0);
        assert!(panel.next_image());
        assert_eq!(panel.image_index(), 1);
        assert!(panel.next_image());
        assert!(panel.next_image());
        assert_eq!(panel.image_index(), 0);
        assert!(panel.prev_image());
        assert_eq!(panel.image_index(), 2);
    }

    #[test]
    fn image_navigation_noop_with_single_image() {
        let mut panel = DetailPanel::new();
        panel.show_listing(Some(listing_with("a", vec!["a.jpg"], None)));
        assert!(!panel.next_image());
        assert!(!panel.prev_image());
        assert_eq!(panel.image_index(), 0);
    }

    #[test]
    fn has_ai_notes_true_when_notable_or_reason_present() {
        let mut panel = DetailPanel::new();
        panel.show_listing(Some(listing_with("a", vec![], None)));
        assert!(!panel.has_ai_notes());

        panel.show_listing(Some(listing_with(
            "b",
            vec![],
            Some(r#"{"notable": "", "reason": ""}"#),
        )));
        assert!(!panel.has_ai_notes());

        panel.show_listing(Some(listing_with(
            "c",
            vec![],
            Some(r#"{"notable": "rare", "reason": ""}"#),
        )));
        assert!(panel.has_ai_notes());
    }

    #[test]
    fn set_gallery_preserves_current_url_and_hero_path() {
        let mut panel = DetailPanel::new();
        panel.show_listing(Some(listing_with("a", vec!["thumb.jpg"], None)));
        panel.hero_image_path = Some(PathBuf::from("/tmp/thumb-cached.jpg"));

        panel.set_gallery(vec![
            "full1.jpg".to_string(),
            "thumb.jpg".to_string(),
            "full3.jpg".to_string(),
        ]);

        // Index follows the URL that was already showing...
        assert_eq!(panel.image_index(), 1);
        assert_eq!(panel.current_image_url(), Some("thumb.jpg"));
        // ...and the hero path is left alone; only the caller (on an
        // actual resolved download) is allowed to swap it.
        assert!(panel.hero_image_path.is_some());
    }

    #[test]
    fn set_gallery_falls_back_to_first_when_current_url_gone() {
        let mut panel = DetailPanel::new();
        panel.show_listing(Some(listing_with("a", vec!["thumb.jpg"], None)));
        panel.set_gallery(vec!["full1.jpg".to_string(), "full2.jpg".to_string()]);
        assert_eq!(panel.image_index(), 0);
    }

    #[test]
    fn set_gallery_ignores_empty_images() {
        let mut panel = DetailPanel::new();
        panel.show_listing(Some(listing_with("a", vec!["thumb.jpg"], None)));
        panel.set_gallery(vec![]);
        assert_eq!(panel.current_image_url(), Some("thumb.jpg"));
    }

    #[test]
    fn image_band_scales_with_terminal_and_leaves_text_room() {
        let mut panel = DetailPanel::new();
        panel.show_listing(Some(listing_with("a", vec!["a.jpg"], None)));
        panel.hero_image_path = Some(PathBuf::from("/tmp/fake.jpg"));

        // inner height = area.height - 2 (top/bottom border)
        let area = Rect::new(0, 0, 40, 12); // inner = 10
        let mut buf = Buffer::empty(area);
        let image_area = panel.render(area, &mut buf, false);
        let band = image_area.expect("band present at 10 inner rows").height;
        assert!((4..=16).contains(&band));
        assert!(band <= 10u16.saturating_sub(6));

        let area = Rect::new(0, 0, 40, 42); // inner = 40
        let mut buf = Buffer::empty(area);
        let image_area = panel.render(area, &mut buf, false);
        let band = image_area.expect("band present at 40 inner rows").height;
        assert!((4..=16).contains(&band));
        // Text always keeps at least 6 rows.
        assert!(band <= 40u16 - 6);
    }

    #[test]
    fn image_band_skipped_on_short_terminal() {
        let mut panel = DetailPanel::new();
        panel.show_listing(Some(listing_with("a", vec!["a.jpg"], None)));
        panel.hero_image_path = Some(PathBuf::from("/tmp/fake.jpg"));

        // inner height = 7, below the 10-row floor — no image band, but the
        // panel must still render (returns None, not a zero-height Rect).
        let area = Rect::new(0, 0, 40, 9);
        let mut buf = Buffer::empty(area);
        assert!(panel.render(area, &mut buf, false).is_none());
    }
}
