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

    // Image nav + keybind hints
    let mut hint_spans: Vec<Span> = Vec::new();
    if img_total > 1 {
        hint_spans.push(Span::styled("<", Style::default().fg(colors::TEXT_DIMMER)));
        hint_spans.push(Span::raw(" "));
        hint_spans.push(Span::styled(
            format!("{}/{}", img_index + 1, img_total),
            Style::default().fg(colors::TEXT_PRIMARY),
        ));
        hint_spans.push(Span::raw(" "));
        hint_spans.push(Span::styled(">", Style::default().fg(colors::TEXT_DIMMER)));
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
                    .map_or(false, |s| !s.is_empty())
                    || v.get("notable")
                        .and_then(|n| n.as_str())
                        .map_or(false, |s| !s.is_empty())
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

    /// Set full gallery images (from detail page scrape).
    pub fn set_gallery(&mut self, images: Vec<String>) {
        if !images.is_empty() {
            self.images = images;
            self.img_idx = 0;
            self.hero_image_path = None;
        }
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

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
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
            .border_style(Style::default().fg(colors::INDICATOR_ACTIVE))
            .style(Style::default().bg(colors::BG_DETAIL));

        let inner = block.inner(area);
        Widget::render(block, area, buf);

        if inner.width == 0 || inner.height == 0 {
            return;
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
            }
            Some(listing) => {
                // Split inner area: hero image area (if we have an image path) + text
                let (text_area, _image_area) = if self.hero_image_path.is_some() {
                    let chunks = Layout::vertical([
                        Constraint::Length(18),
                        Constraint::Min(0),
                    ])
                    .split(inner);
                    (chunks[1], Some(chunks[0]))
                } else {
                    (inner, None)
                };

                // Render image placeholder when no resolved path
                // (actual ratatui-image rendering would be done by the app's event loop)

                let text = render_listing_text(listing, self.img_idx, self.images.len());
                let content = Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .style(Style::default().bg(colors::BG_DETAIL));
                Widget::render(content, text_area, buf);
            }
        }
    }
}
