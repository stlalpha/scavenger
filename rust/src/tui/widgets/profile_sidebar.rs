use std::collections::{HashMap, HashSet};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, StatefulWidget},
};

use crate::models::Profile;
use crate::tui::colors;

const SRC_BADGES: &[(&str, &str, ratatui::style::Color)] = &[
    ("ebay", "eb", colors::YELLOW),
    ("craigslist", "cl", colors::PINK),
    ("facebook", "fb", colors::BLUE),
];

fn source_badge(source: &str) -> Span<'static> {
    for &(name, abbr, color) in SRC_BADGES {
        if source == name {
            return Span::styled(abbr.to_string(), Style::default().fg(color));
        }
    }
    Span::styled("??", Style::default().fg(colors::TEXT_MUTED))
}

/// Profile sidebar showing the list of search profiles with unread counts.
pub struct ProfileSidebar {
    profiles: Vec<Profile>,
    stats: HashMap<String, usize>,
    daemon_profiles: HashSet<String>,
    pub state: ListState,
}

impl ProfileSidebar {
    pub fn new(profiles: Vec<Profile>) -> Self {
        let mut state = ListState::default();
        if !profiles.is_empty() {
            state.select(Some(0));
        }
        Self {
            profiles,
            stats: HashMap::new(),
            daemon_profiles: HashSet::new(),
            state,
        }
    }

    pub fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn selected_profile(&self) -> Option<&Profile> {
        self.state.selected().and_then(|i| self.profiles.get(i))
    }

    pub fn selected_profile_id(&self) -> Option<&str> {
        self.selected_profile().map(|p| p.id.as_str())
    }

    pub fn update_stats(&mut self, stats: HashMap<String, usize>) {
        self.stats = stats;
    }

    pub fn set_daemon_profiles(&mut self, ids: Vec<String>) {
        self.daemon_profiles = ids.into_iter().collect();
    }

    pub fn select_next(&mut self) {
        if self.profiles.is_empty() {
            return;
        }
        let i = self.state.selected().map_or(0, |i| {
            if i + 1 < self.profiles.len() { i + 1 } else { i }
        });
        self.state.select(Some(i));
    }

    pub fn select_prev(&mut self) {
        if self.profiles.is_empty() {
            return;
        }
        let i = self.state.selected().map_or(0, |i| i.saturating_sub(1));
        self.state.select(Some(i));
    }

    pub fn add_profile(&mut self, profile: Profile) {
        self.profiles.push(profile);
        if self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    pub fn rebuild(&mut self, profiles: Vec<Profile>) {
        let selected_id = self.selected_profile_id().map(str::to_string);
        self.profiles = profiles;
        // Try to preserve selection by id
        let idx = selected_id
            .and_then(|id| self.profiles.iter().position(|p| p.id == id))
            .unwrap_or(0);
        if self.profiles.is_empty() {
            self.state.select(None);
        } else {
            self.state.select(Some(idx.min(self.profiles.len() - 1)));
        }
    }

    pub fn unread(&self, profile_id: &str) -> usize {
        self.stats.get(profile_id).copied().unwrap_or(0)
    }

    fn render_label(&self, profile: &Profile) -> Line<'static> {
        let count = self.unread(&profile.id);

        let srcs: Vec<Span<'static>> = profile
            .sources
            .iter()
            .enumerate()
            .flat_map(|(i, s)| {
                let mut spans = Vec::new();
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(source_badge(s));
                spans
            })
            .collect();

        if !profile.enabled {
            let spans = vec![
                Span::raw("  "),
                Span::styled(profile.name.clone(), Style::default().fg(colors::TEXT_MUTED)),
                Span::styled(" off", Style::default().fg(colors::TEXT_DISABLED)),
            ];
            return Line::from(spans);
        }

        if !self.daemon_profiles.is_empty() && !self.daemon_profiles.contains(&profile.id) {
            let mut spans = vec![
                Span::raw("  "),
                Span::styled("▪", Style::default().fg(colors::ORANGE)),
                Span::raw(" "),
                Span::raw(profile.name.clone()),
            ];
            if count > 0 {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    count.to_string(),
                    Style::default().fg(colors::GREEN).add_modifier(Modifier::BOLD),
                ));
            }
            spans.push(Span::raw(" "));
            spans.extend(srcs);
            return Line::from(spans);
        }

        let indicator = if count > 0 {
            Span::styled("▸", Style::default().fg(colors::GREEN))
        } else {
            Span::styled("▸", Style::default().fg(colors::INDICATOR_ACTIVE))
        };

        let mut spans = vec![
            Span::raw(" "),
            indicator,
            Span::raw(" "),
            Span::raw(profile.name.clone()),
        ];

        if count > 0 {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                count.to_string(),
                Style::default().fg(colors::GREEN).add_modifier(Modifier::BOLD),
            ));
        }

        spans.push(Span::raw(" "));
        spans.extend(srcs);

        Line::from(spans)
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let items: Vec<ListItem> = self
            .profiles
            .iter()
            .map(|p| ListItem::new(self.render_label(p)))
            .collect();

        let list = List::new(items)
            .block(
                Block::bordered()
                    .title(Span::styled(
                        " PROFILES",
                        Style::default()
                            .fg(colors::GREEN)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .border_style(Style::default().fg(colors::INDICATOR_ACTIVE))
                    .style(Style::default().bg(colors::BG_SIDEBAR)),
            )
            .highlight_style(Style::default().bg(colors::BG_SELECTED));

        StatefulWidget::render(list, area, buf, &mut self.state);
    }
}
