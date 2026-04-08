use chrono::{DateTime, Utc};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, StatefulWidget, Widget},
};

use crate::models::{Listing, ListingStatus};
use crate::tui::colors;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Newest,
    Oldest,
    PriceLow,
    PriceHigh,
    Relevance,
    Source,
}

impl SortKey {
    pub const ALL: [SortKey; 6] = [
        SortKey::Newest,
        SortKey::Oldest,
        SortKey::PriceLow,
        SortKey::PriceHigh,
        SortKey::Relevance,
        SortKey::Source,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Newest => "newest",
            SortKey::Oldest => "oldest",
            SortKey::PriceLow => "price \u{2191}",
            SortKey::PriceHigh => "price \u{2193}",
            SortKey::Relevance => "score",
            SortKey::Source => "source",
        }
    }

    pub fn next(self) -> SortKey {
        let idx = SortKey::ALL.iter().position(|&k| k == self).unwrap_or(0);
        SortKey::ALL[(idx + 1) % SortKey::ALL.len()]
    }
}

fn sort_listings(listings: &mut [Listing], key: SortKey) {
    match key {
        SortKey::Newest => listings.sort_by(|a, b| b.first_seen.cmp(&a.first_seen)),
        SortKey::Oldest => listings.sort_by(|a, b| a.first_seen.cmp(&b.first_seen)),
        SortKey::PriceLow => listings.sort_by(|a, b| {
            let ak = (a.price.is_none(), a.price.unwrap_or(0.0));
            let bk = (b.price.is_none(), b.price.unwrap_or(0.0));
            ak.partial_cmp(&bk).unwrap_or(std::cmp::Ordering::Equal)
        }),
        SortKey::PriceHigh => listings.sort_by(|a, b| {
            let ak = (a.price.is_none(), -(a.price.unwrap_or(0.0)));
            let bk = (b.price.is_none(), -(b.price.unwrap_or(0.0)));
            ak.partial_cmp(&bk).unwrap_or(std::cmp::Ordering::Equal)
        }),
        SortKey::Relevance => {
            listings.sort_by(|a, b| {
                b.relevance_score
                    .partial_cmp(&a.relevance_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        SortKey::Source => listings.sort_by(|a, b| {
            (&a.source_id, std::cmp::Reverse(&a.first_seen))
                .cmp(&(&b.source_id, std::cmp::Reverse(&b.first_seen)))
        }),
    }
}

fn age_str(dt: &DateTime<Utc>) -> String {
    let secs = Utc::now().signed_duration_since(dt).num_seconds().max(0);
    if secs < 60 {
        "now".into()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

fn has_notable(listing: &Listing) -> bool {
    listing
        .ai_evaluation
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("notable")?.as_str().map(|s| !s.is_empty()))
        .unwrap_or(false)
}


fn render_card(listing: &Listing) -> Vec<Line<'static>> {
    let icon = match listing.status {
        ListingStatus::New => Span::styled("●", Style::default().fg(colors::BLUE).add_modifier(Modifier::BOLD)),
        ListingStatus::Seen => Span::styled("·", Style::default().fg(colors::TEXT_DIMMEST)),
        ListingStatus::Saved => Span::styled("★", Style::default().fg(colors::GREEN).add_modifier(Modifier::BOLD)),
        ListingStatus::Dismissed => Span::styled("✕", Style::default().fg(colors::TEXT_DISABLED)),
        ListingStatus::Snoozed => Span::styled("◑", Style::default().fg(colors::YELLOW)),
    };

    let notable_span = if has_notable(listing) {
        Span::styled(" !", Style::default().fg(colors::PINK).add_modifier(Modifier::BOLD))
    } else {
        Span::raw("")
    };

    let title_color = match listing.status {
        ListingStatus::Seen | ListingStatus::Dismissed => colors::TEXT_DIM,
        _ => colors::TEXT_PRIMARY,
    };

    let title = Span::styled(listing.title.clone(), Style::default().fg(title_color));

    let price = match listing.price {
        Some(p) => Span::styled(
            format!("${:.0}", p),
            Style::default().fg(colors::ORANGE).add_modifier(Modifier::BOLD),
        ),
        None => Span::styled("--", Style::default().fg(colors::TEXT_DIMMEST)),
    };

    let src_abbr = if listing.source_id.len() >= 2 {
        listing.source_id[..2].to_uppercase()
    } else {
        listing.source_id.to_uppercase()
    };
    let src = Span::styled(src_abbr, Style::default().fg(colors::src_color(&listing.source_id)));

    let age = Span::styled(age_str(&listing.first_seen), Style::default().fg(colors::TEXT_DIMMER));

    let line1 = Line::from(vec![icon, notable_span, Span::raw(" "), title]);
    let line2 = Line::from(vec![
        Span::raw("  "),
        price,
        Span::raw("  "),
        src,
        Span::raw("  "),
        age,
    ]);

    vec![line1, line2]
}

/// Scrollable feed of listing cards with sort and cursor tracking.
pub struct ResultsFeed {
    raw_listings: Vec<Listing>,
    sorted_listings: Vec<Listing>,
    fingerprint: String,
    sort_key: SortKey,
    awaiting_poll: bool,
    pub state: ListState,
}

impl ResultsFeed {
    pub fn new() -> Self {
        Self {
            raw_listings: Vec::new(),
            sorted_listings: Vec::new(),
            fingerprint: String::new(),
            sort_key: SortKey::Newest,
            awaiting_poll: true,
            state: ListState::default(),
        }
    }

    pub fn listing_count(&self) -> usize {
        self.sorted_listings.len()
    }

    pub fn focused_listing(&self) -> Option<&Listing> {
        self.state
            .selected()
            .and_then(|i| self.sorted_listings.get(i))
    }

    pub fn invalidate_fingerprint(&mut self) {
        self.fingerprint = "__stale__".to_string();
        self.awaiting_poll = true;
    }

    /// Returns true if the listing set changed.
    pub fn update_listings(&mut self, listings: Vec<Listing>, from_poll: bool) -> bool {
        if from_poll {
            self.awaiting_poll = false;
        }
        let fp: String = listings
            .iter()
            .map(|l| format!("{}:{:?}", l.id, l.status))
            .collect::<Vec<_>>()
            .join("|");
        if fp == self.fingerprint {
            return false;
        }
        self.fingerprint = fp;
        self.raw_listings = listings;
        self.resort();
        true
    }

    fn resort(&mut self) {
        self.sorted_listings = self.raw_listings.clone();
        sort_listings(&mut self.sorted_listings, self.sort_key);
        // Clamp cursor
        let max = self.sorted_listings.len().saturating_sub(1);
        match self.state.selected() {
            Some(i) if i > max => self.state.select(Some(max)),
            None if !self.sorted_listings.is_empty() => self.state.select(Some(0)),
            _ => {}
        }
    }

    pub fn cycle_sort(&mut self) {
        self.sort_key = self.sort_key.next();
        self.fingerprint.clear();
        self.resort();
    }

    pub fn sort_key(&self) -> SortKey {
        self.sort_key
    }

    pub fn select_next(&mut self) {
        if self.sorted_listings.is_empty() {
            return;
        }
        let i = self.state.selected().map_or(0, |i| {
            (i + 1).min(self.sorted_listings.len() - 1)
        });
        self.state.select(Some(i));
    }

    pub fn select_prev(&mut self) {
        if self.sorted_listings.is_empty() {
            return;
        }
        let i = self.state.selected().map_or(0, |i| i.saturating_sub(1));
        self.state.select(Some(i));
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let new_count = self
            .sorted_listings
            .iter()
            .filter(|l| l.status == ListingStatus::New)
            .count();

        let header_suffix = if !self.sorted_listings.is_empty() && new_count > 0 {
            format!(" {} new {}", new_count, self.sort_key.label())
        } else if !self.sorted_listings.is_empty() {
            format!(" {} {}", self.sorted_listings.len(), self.sort_key.label())
        } else if self.awaiting_poll {
            " polling...".to_string()
        } else {
            " empty".to_string()
        };

        let title_line = Line::from(vec![
            Span::styled(
                " LISTINGS",
                Style::default()
                    .fg(colors::BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(header_suffix, Style::default().fg(colors::TEXT_DIMMER)),
        ]);

        if self.sorted_listings.is_empty() {
            let msg = if self.awaiting_poll {
                "  waiting for results\u{2026}"
            } else {
                "  no listings found"
            };
            let items = vec![ListItem::new(Span::styled(
                msg,
                Style::default()
                    .fg(colors::TEXT_MUTED)
                    .add_modifier(Modifier::ITALIC),
            ))];
            let list = List::new(items).block(
                Block::bordered()
                    .title(title_line)
                    .border_style(Style::default().fg(colors::INDICATOR_ACTIVE))
                    .style(Style::default().bg(colors::BG_FEED)),
            );
            Widget::render(list, area, buf);
            return;
        }

        let items: Vec<ListItem> = self
            .sorted_listings
            .iter()
            .map(|l| ListItem::new(render_card(l)))
            .collect();

        let list = List::new(items)
            .block(
                Block::bordered()
                    .title(title_line)
                    .border_style(Style::default().fg(colors::INDICATOR_ACTIVE))
                    .style(Style::default().bg(colors::BG_FEED)),
            )
            .highlight_style(Style::default().bg(colors::BG_HIGHLIGHT));

        StatefulWidget::render(list, area, buf, &mut self.state);
    }
}
