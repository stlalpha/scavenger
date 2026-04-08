use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Widget, Wrap};

use crate::models::{Listing, ListingStatus, Profile};
use crate::tui::widgets::log_panel::{LogPanelState, LogPanelWidget};
use crate::tui::widgets::splitter::{
    HSplitterState, HSplitterWidget, VSplitterState, VSplitterWidget,
};
use crate::tui::widgets::status_bar::{StatusBarState, StatusBarWidget};
use crate::tui::FocusedPanel;

const CLR_BG: Color = Color::Rgb(0x16, 0x16, 0x16);
const CLR_PANEL_BG: Color = Color::Rgb(0x1a, 0x1a, 0x1a);
const CLR_ACCENT: Color = Color::Rgb(0xfd, 0x97, 0x1f);
const CLR_GREEN: Color = Color::Rgb(0xa6, 0xe2, 0x2e);
const CLR_DATA: Color = Color::Rgb(0x66, 0xd9, 0xef);
const CLR_DIM: Color = Color::Rgb(0x3a, 0x3a, 0x3a);
const CLR_MUTED: Color = Color::Rgb(0x75, 0x71, 0x5e);
const CLR_FG: Color = Color::Rgb(0xf8, 0xf8, 0xf2);

/// Compute the main layout areas.
///
/// ```text
/// ┌──────────┬─────────────────┬──────────────────┐
/// │ PROFILES │    LISTINGS     │     DETAIL       │
/// │ (sidebar)│    (feed)       │    (panel)       │
/// ├──────────┴─────────────────┴──────────────────┤
/// │                    LOG (variable)              │
/// ├────────────────────────────────────────────────┤
/// │                 STATUS BAR (1 row)             │
/// └────────────────────────────────────────────────┘
/// ```
pub struct MainLayout {
    pub sidebar: Rect,
    pub vsplit1: Rect,
    pub feed: Rect,
    pub vsplit2: Rect,
    pub detail: Rect,
    pub hsplit: Rect,
    pub log: Rect,
    pub status_bar: Rect,
}

impl MainLayout {
    pub fn compute(
        area: Rect,
        sidebar_width: u16,
        log_height: u16,
    ) -> Self {
        // Top-level vertical: [content, hsplit(1), log, status_bar(1)]
        let vert = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),           // content area
                Constraint::Length(1),         // horizontal splitter
                Constraint::Length(log_height),// log panel
                Constraint::Length(1),         // status bar
            ])
            .split(area);

        let content_area = vert[0];
        let hsplit_area = vert[1];
        let log_area = vert[2];
        let status_area = vert[3];

        // Horizontal split of content: [sidebar, vsplit(1), feed, vsplit(1), detail]
        // Left side = sidebar_width. Detail gets ~40% of what remains.
        let left_total = content_area.width.saturating_sub(1); // minus 1 for vsplit2
        let detail_width = left_total * 40 / 100;
        let left_width = left_total.saturating_sub(detail_width);

        let horiz = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(left_width),
                Constraint::Length(1),       // vsplit2
                Constraint::Min(20),         // detail
            ])
            .split(content_area);

        let left_area = horiz[0];
        let vsplit2_area = horiz[1];
        let detail_area = horiz[2];

        // Split left area into [sidebar, vsplit1(1), feed]
        let left_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(sidebar_width),
                Constraint::Length(1),  // vsplit1
                Constraint::Min(20),   // feed
            ])
            .split(left_area);

        Self {
            sidebar: left_cols[0],
            vsplit1: left_cols[1],
            feed: left_cols[2],
            vsplit2: vsplit2_area,
            detail: detail_area,
            hsplit: hsplit_area,
            log: log_area,
            status_bar: status_area,
        }
    }
}

/// Render the full main screen into the buffer.
pub fn render_main_screen(
    area: Rect,
    buf: &mut Buffer,
    profiles: &[Profile],
    active_profile: Option<&str>,
    profile_stats: &std::collections::HashMap<String, u32>,
    listings: &[Listing],
    selected_listing: Option<usize>,
    focused: FocusedPanel,
    vsplit1: &VSplitterState,
    vsplit2: &VSplitterState,
    hsplit: &HSplitterState,
    log_state: &LogPanelState,
    status_state: &StatusBarState,
) {
    let layout = MainLayout::compute(area, vsplit1.left_width, hsplit.bottom_height);

    // -- Profile sidebar --
    render_profile_sidebar(
        layout.sidebar,
        buf,
        profiles,
        active_profile,
        profile_stats,
        focused == FocusedPanel::Profiles,
    );

    // -- Splitters --
    VSplitterWidget::new(vsplit1).render(layout.vsplit1, buf);
    VSplitterWidget::new(vsplit2).render(layout.vsplit2, buf);
    HSplitterWidget::new(hsplit).render(layout.hsplit, buf);

    // -- Results feed --
    render_results_feed(
        layout.feed,
        buf,
        listings,
        selected_listing,
        focused == FocusedPanel::Feed,
    );

    // -- Detail panel --
    let detail_listing = selected_listing.and_then(|i| listings.get(i));
    render_detail_panel(
        layout.detail,
        buf,
        detail_listing,
        focused == FocusedPanel::Detail,
    );

    // -- Log --
    LogPanelWidget::new(log_state, focused == FocusedPanel::Log).render(layout.log, buf);

    // -- Status bar --
    StatusBarWidget::new(status_state).render(layout.status_bar, buf);
}

fn render_profile_sidebar(
    area: Rect,
    buf: &mut Buffer,
    profiles: &[Profile],
    active_profile: Option<&str>,
    stats: &std::collections::HashMap<String, u32>,
    focused: bool,
) {
    let border_color = if focused { CLR_ACCENT } else { Color::Rgb(0x22, 0x22, 0x22) };
    let block = Block::default()
        .title(Span::styled(
            " PROFILES",
            Style::default().fg(CLR_GREEN).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(CLR_BG));

    let inner = block.inner(area);
    block.render(area, buf);

    let items: Vec<ListItem> = profiles
        .iter()
        .map(|p| {
            let is_active = active_profile == Some(p.id.as_str());
            let count = stats.get(&p.id).copied().unwrap_or(0);
            let style = if is_active {
                Style::default().fg(CLR_GREEN).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(CLR_MUTED)
            };
            let prefix = if is_active { "▸ " } else { "  " };
            let count_str = if count > 0 {
                format!(" ({count})")
            } else {
                String::new()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{prefix}{}", p.name), style),
                Span::styled(count_str, Style::default().fg(CLR_DATA)),
            ]))
        })
        .collect();

    let list = List::new(items);
    list.render(inner, buf);
}

fn render_results_feed(
    area: Rect,
    buf: &mut Buffer,
    listings: &[Listing],
    selected: Option<usize>,
    focused: bool,
) {
    let border_color = if focused { CLR_ACCENT } else { Color::Rgb(0x22, 0x22, 0x22) };
    let block = Block::default()
        .title(Span::styled(
            " LISTINGS",
            Style::default().fg(CLR_DATA).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::NONE)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(CLR_PANEL_BG));

    let inner = block.inner(area);
    block.render(area, buf);

    if listings.is_empty() {
        let msg = Paragraph::new("No listings yet")
            .style(Style::default().fg(CLR_DIM));
        msg.render(inner, buf);
        return;
    }

    let items: Vec<ListItem> = listings
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let is_selected = selected == Some(i);
            let status_indicator = match l.status {
                ListingStatus::New => Span::styled("● ", Style::default().fg(CLR_GREEN)),
                ListingStatus::Saved => Span::styled("★ ", Style::default().fg(CLR_ACCENT)),
                ListingStatus::Seen => Span::styled("  ", Style::default().fg(CLR_DIM)),
                ListingStatus::Dismissed => Span::styled("✕ ", Style::default().fg(CLR_DIM)),
                ListingStatus::Snoozed => Span::styled("◷ ", Style::default().fg(CLR_MUTED)),
            };
            let title_style = if is_selected {
                Style::default().fg(CLR_FG).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(CLR_MUTED)
            };
            let price_str = l
                .price
                .map(|p| format!(" ${p:.0}"))
                .unwrap_or_default();
            ListItem::new(Line::from(vec![
                status_indicator,
                Span::styled(&l.title, title_style),
                Span::styled(price_str, Style::default().fg(CLR_GREEN)),
            ]))
        })
        .collect();

    let list = List::new(items);
    list.render(inner, buf);
}

fn render_detail_panel(
    area: Rect,
    buf: &mut Buffer,
    listing: Option<&Listing>,
    focused: bool,
) {
    let border_color = if focused { CLR_ACCENT } else { Color::Rgb(0x22, 0x22, 0x22) };
    let block = Block::default()
        .title(Span::styled(
            " DETAIL",
            Style::default().fg(CLR_ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::NONE)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(CLR_PANEL_BG));

    let inner = block.inner(area);
    block.render(area, buf);

    let Some(listing) = listing else {
        let msg = Paragraph::new("Select a listing")
            .style(Style::default().fg(CLR_DIM));
        msg.render(inner, buf);
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        &listing.title,
        Style::default().fg(CLR_FG).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if let Some(price) = listing.price {
        lines.push(Line::from(vec![
            Span::styled("Price: ", Style::default().fg(CLR_MUTED)),
            Span::styled(
                format!("${price:.2}"),
                Style::default().fg(CLR_GREEN).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    if let Some(ref loc) = listing.location {
        lines.push(Line::from(vec![
            Span::styled("Location: ", Style::default().fg(CLR_MUTED)),
            Span::styled(loc.as_str(), Style::default().fg(CLR_FG)),
        ]));
    }

    if let Some(ref cond) = listing.condition {
        lines.push(Line::from(vec![
            Span::styled("Condition: ", Style::default().fg(CLR_MUTED)),
            Span::styled(cond.as_str(), Style::default().fg(CLR_FG)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Source: ", Style::default().fg(CLR_MUTED)),
        Span::styled(&listing.source_id, Style::default().fg(CLR_DATA)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Score: ", Style::default().fg(CLR_MUTED)),
        Span::styled(
            format!("{:.0}", listing.relevance_score),
            Style::default().fg(CLR_ACCENT),
        ),
    ]));

    if !listing.description.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            &listing.description,
            Style::default().fg(CLR_MUTED),
        )));
    }

    // Keybind hints at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("o", Style::default().fg(CLR_ACCENT)),
        Span::styled("pen  ", Style::default().fg(CLR_DIM)),
        Span::styled("s", Style::default().fg(CLR_ACCENT)),
        Span::styled("ave  ", Style::default().fg(CLR_DIM)),
        Span::styled("d", Style::default().fg(CLR_ACCENT)),
        Span::styled("ismiss  ", Style::default().fg(CLR_DIM)),
        Span::styled("n", Style::default().fg(CLR_ACCENT)),
        Span::styled("snooze", Style::default().fg(CLR_DIM)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    paragraph.render(inner, buf);
}
