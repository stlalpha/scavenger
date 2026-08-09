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

/// Fixed render height of one listing card — the title line plus the
/// price/source/age line. Hit-testing and scroll math both depend on this
/// staying in sync with `render_card`'s output.
pub const CARD_HEIGHT: u16 = 2;

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

    let src_abbr: String = listing.source_id.chars().take(2).collect::<String>().to_uppercase();
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

impl Default for ResultsFeed {
    fn default() -> Self {
        Self::new()
    }
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
        // Include AI presence and price in the fingerprint: the daemon
        // attaches ai_evaluation (and can revise price) after a listing
        // already exists without changing id/status, and render_card draws
        // the "!" notable marker from ai_evaluation — an id:status-only
        // fingerprint would keep showing the pre-AI cards.
        let fp: String = listings
            .iter()
            .map(|l| {
                format!(
                    "{}:{:?}:{}:{:?}",
                    l.id,
                    l.status,
                    l.ai_evaluation.is_some(),
                    l.price
                )
            })
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

    /// Re-sort and restore the selection by listing id rather than raw row
    /// index — otherwise a poll landing mid-read (e.g. Newest sort
    /// prepending a new row) silently swaps the listing under the cursor.
    fn resort(&mut self) {
        let selected_id = self.focused_listing().map(|l| l.id.clone());

        self.sorted_listings = self.raw_listings.clone();
        sort_listings(&mut self.sorted_listings, self.sort_key);

        if self.sorted_listings.is_empty() {
            self.state.select(None);
            return;
        }

        if let Some(id) = selected_id {
            if let Some(idx) = self.sorted_listings.iter().position(|l| l.id == id) {
                self.state.select(Some(idx));
                return;
            }
        }

        // Previously-selected listing is gone (dismissed elsewhere) or
        // nothing was selected — clamp to bounds instead of losing position.
        let max = self.sorted_listings.len() - 1;
        match self.state.selected() {
            Some(i) => self.state.select(Some(i.min(max))),
            None => self.state.select(Some(0)),
        }
    }

    /// Map a mouse row (0-based, relative to the list's inner content area)
    /// to a listing index — honors the current scroll offset and the fixed
    /// `CARD_HEIGHT`, unlike a naive 1:1 row mapping which silently selects
    /// the wrong listing on any card past the first or any scrolled view.
    /// Returns `None` if the row falls past the last rendered card.
    pub fn hit_test(&self, row: u16) -> Option<usize> {
        let idx = self.state.offset() + (row / CARD_HEIGHT) as usize;
        if idx < self.sorted_listings.len() {
            Some(idx)
        } else {
            None
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

    pub fn render(&mut self, area: Rect, buf: &mut Buffer, focused: bool) {
        let border_color = if focused {
            colors::ORANGE
        } else {
            colors::INDICATOR_ACTIVE
        };

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
                    .border_style(Style::default().fg(border_color))
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

        // Same selection wash convention as the sidebar: brighter when this
        // panel actually has focus, dimmer otherwise.
        let highlight_bg = if focused {
            colors::BG_SELECTED
        } else {
            colors::BG_HIGHLIGHT
        };
        let list = List::new(items)
            .block(
                Block::bordered()
                    .title(title_line)
                    .border_style(Style::default().fg(border_color))
                    .style(Style::default().bg(colors::BG_FEED)),
            )
            .highlight_style(Style::default().bg(highlight_bg));

        StatefulWidget::render(list, area, buf, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ListingStatus;

    fn listing(id: &str, price: Option<f64>, score: f64, source: &str, secs_ago: i64) -> Listing {
        Listing {
            id: id.to_string(),
            profile_id: "p1".to_string(),
            source_id: source.to_string(),
            title: id.to_string(),
            description: String::new(),
            price,
            currency: "USD".to_string(),
            condition: None,
            url: format!("https://example.com/{id}"),
            image_urls: Vec::new(),
            location: None,
            first_seen: Utc::now() - chrono::Duration::seconds(secs_ago),
            last_seen: Utc::now(),
            relevance_score: score,
            status: ListingStatus::New,
            ai_evaluation: None,
        }
    }

    #[test]
    fn sort_key_cycles_through_all_six() {
        let mut key = SortKey::Newest;
        let mut seen = vec![key];
        for _ in 0..5 {
            key = key.next();
            seen.push(key);
        }
        assert_eq!(seen, SortKey::ALL.to_vec());
        assert_eq!(key.next(), SortKey::Newest);
    }

    #[test]
    fn sort_by_price_low_puts_none_last() {
        let mut listings = vec![
            listing("a", None, 0.0, "ebay", 0),
            listing("b", Some(50.0), 0.0, "ebay", 0),
            listing("c", Some(10.0), 0.0, "ebay", 0),
        ];
        sort_listings(&mut listings, SortKey::PriceLow);
        assert_eq!(
            listings.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            vec!["c", "b", "a"]
        );
    }

    #[test]
    fn sort_by_price_high_puts_none_last() {
        let mut listings = vec![
            listing("a", None, 0.0, "ebay", 0),
            listing("b", Some(50.0), 0.0, "ebay", 0),
            listing("c", Some(10.0), 0.0, "ebay", 0),
        ];
        sort_listings(&mut listings, SortKey::PriceHigh);
        assert_eq!(
            listings.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "c", "a"]
        );
    }

    #[test]
    fn sort_by_relevance_descending() {
        let mut listings = vec![
            listing("a", None, 10.0, "ebay", 0),
            listing("b", None, 90.0, "ebay", 0),
            listing("c", None, 50.0, "ebay", 0),
        ];
        sort_listings(&mut listings, SortKey::Relevance);
        assert_eq!(
            listings.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "c", "a"]
        );
    }

    #[test]
    fn age_str_buckets_by_magnitude() {
        assert_eq!(age_str(&(Utc::now() - chrono::Duration::seconds(10))), "now");
        assert_eq!(age_str(&(Utc::now() - chrono::Duration::minutes(5))), "5m");
        assert_eq!(age_str(&(Utc::now() - chrono::Duration::hours(3))), "3h");
        assert_eq!(age_str(&(Utc::now() - chrono::Duration::days(2))), "2d");
    }

    #[test]
    fn has_notable_reads_ai_evaluation_json() {
        let mut l = listing("a", None, 0.0, "ebay", 0);
        assert!(!has_notable(&l));
        l.ai_evaluation = Some(r#"{"notable": "", "reason": "meh"}"#.to_string());
        assert!(!has_notable(&l));
        l.ai_evaluation = Some(r#"{"notable": "rare find", "reason": "meh"}"#.to_string());
        assert!(has_notable(&l));
    }

    #[test]
    fn update_listings_fingerprint_dedupes_unchanged_sets() {
        let mut feed = ResultsFeed::new();
        let listings = vec![listing("a", Some(10.0), 0.0, "ebay", 0)];
        assert!(feed.update_listings(listings.clone(), true));
        assert!(!feed.update_listings(listings, true));
    }

    #[test]
    fn hit_test_maps_two_row_cards_at_zero_offset() {
        let mut feed = ResultsFeed::new();
        feed.update_listings(
            vec![
                listing("a", None, 0.0, "ebay", 0),
                listing("b", None, 0.0, "ebay", 0),
                listing("c", None, 0.0, "ebay", 0),
            ],
            true,
        );
        assert_eq!(feed.hit_test(0), Some(0));
        assert_eq!(feed.hit_test(1), Some(0));
        assert_eq!(feed.hit_test(2), Some(1));
        assert_eq!(feed.hit_test(3), Some(1));
        assert_eq!(feed.hit_test(4), Some(2));
        assert_eq!(feed.hit_test(5), Some(2));
        assert_eq!(feed.hit_test(6), None); // past the last card
    }

    #[test]
    fn hit_test_accounts_for_scroll_offset() {
        let mut feed = ResultsFeed::new();
        feed.update_listings(
            (0..10)
                .map(|i| listing(&i.to_string(), None, 0.0, "ebay", 0))
                .collect(),
            true,
        );
        *feed.state.offset_mut() = 3; // as if scrolled so item 3 renders first
        assert_eq!(feed.hit_test(0), Some(3));
        assert_eq!(feed.hit_test(1), Some(3));
        assert_eq!(feed.hit_test(2), Some(4));
        assert_eq!(feed.hit_test(12), Some(9));
        assert_eq!(feed.hit_test(14), None);
    }

    #[test]
    fn hit_test_none_on_empty_feed() {
        let feed = ResultsFeed::new();
        assert_eq!(feed.hit_test(0), None);
    }

    #[test]
    fn resort_restores_selection_by_id_when_newest_prepends_a_row() {
        let mut feed = ResultsFeed::new();
        feed.update_listings(
            vec![
                listing("old", None, 0.0, "ebay", 100),
                listing("mid", None, 0.0, "ebay", 50),
            ],
            true,
        );
        // Select "mid" (index 1 under Newest: old is older, so "mid" sorts
        // first — pick "old" as the one under the cursor instead).
        let idx = feed
            .sorted_listings
            .iter()
            .position(|l| l.id == "old")
            .unwrap();
        feed.state.select(Some(idx));
        assert_eq!(feed.focused_listing().unwrap().id, "old");

        // A poll prepends a brand-new row ahead of both under Newest sort.
        feed.update_listings(
            vec![
                listing("new", None, 0.0, "ebay", 0),
                listing("old", None, 0.0, "ebay", 100),
                listing("mid", None, 0.0, "ebay", 50),
            ],
            true,
        );

        // Selection must still track listing "old", not row index 0.
        assert_eq!(feed.focused_listing().unwrap().id, "old");
    }

    #[test]
    fn resort_clamps_when_selected_listing_disappears() {
        let mut feed = ResultsFeed::new();
        feed.update_listings(
            vec![
                listing("a", None, 0.0, "ebay", 0),
                listing("b", None, 0.0, "ebay", 0),
                listing("c", None, 0.0, "ebay", 0),
            ],
            true,
        );
        // Select "c" by id rather than a hardcoded index — Newest sort
        // ties on near-identical timestamps aren't deterministic enough
        // to assume a fixed position here.
        let idx = feed.sorted_listings.iter().position(|l| l.id == "c").unwrap();
        feed.state.select(Some(idx));

        feed.update_listings(
            vec![listing("a", None, 0.0, "ebay", 0), listing("b", None, 0.0, "ebay", 0)],
            true,
        );

        // "c" is gone — falls back to a clamped index instead of losing
        // the selection or silently resetting to the top.
        let max = feed.sorted_listings.len() - 1;
        assert_eq!(feed.state.selected(), Some(idx.min(max)));
    }

    #[test]
    fn update_listings_fingerprint_changes_on_status() {
        let mut feed = ResultsFeed::new();
        let mut l = listing("a", Some(10.0), 0.0, "ebay", 0);
        assert!(feed.update_listings(vec![l.clone()], true));
        l.status = ListingStatus::Seen;
        assert!(feed.update_listings(vec![l], true));
    }

    #[test]
    fn update_listings_fingerprint_changes_when_ai_eval_attaches() {
        // The daemon attaches ai_evaluation after the row exists, without
        // touching id/status; the feed must re-render so the "!" marker
        // appears rather than keeping the pre-AI card.
        let mut feed = ResultsFeed::new();
        let mut l = listing("a", Some(10.0), 0.0, "ebay", 0);
        assert!(feed.update_listings(vec![l.clone()], true));
        assert!(!feed.update_listings(vec![l.clone()], true), "no change yet");
        l.ai_evaluation = Some(r#"{"relevant":true,"reason":"","notable":"rare","escalate":false}"#.into());
        assert!(feed.update_listings(vec![l], true), "AI attach must re-render");
    }

    #[test]
    fn update_listings_fingerprint_changes_on_price() {
        let mut feed = ResultsFeed::new();
        let mut l = listing("a", Some(10.0), 0.0, "ebay", 0);
        assert!(feed.update_listings(vec![l.clone()], true));
        l.price = Some(9.0);
        assert!(feed.update_listings(vec![l], true), "price change must re-render");
    }

    #[test]
    fn render_card_source_abbrev_never_panics_on_multibyte_source() {
        // Byte-slicing a multibyte source id would panic in the render path.
        let l = listing("a", Some(10.0), 0.0, "é-source", 0);
        let _ = render_card(&l);
    }
}
