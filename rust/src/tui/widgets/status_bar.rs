use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::db::SourceState;
use crate::tui::colors;

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn format_age(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h", seconds / 3600)
    }
}

pub struct StatusBarState {
    pub daemon_up: bool,
    pub new_count: u32,
    pub active_polls: Vec<String>,
    /// (profile_name, source_id) currently being polled — one at a time
    /// under the sequential scheduler, shown so the user knows exactly
    /// which profile the daemon is scraping right now.
    pub polling: Vec<(String, String)>,
    pub source_states: Vec<SourceState>,
    pub last_poll_iso: Option<String>,
    pub poll_interval_sec: u32,
    pub spinner_tick: usize,
    /// (source_id, blocked url) pairs the daemon is currently reporting via
    /// `data.bot_blocks` — non-empty means at least one source needs a
    /// manual unblock in a visible browser (bound to 'x').
    pub bot_blocks: Vec<(String, String)>,
    /// AI evaluation health from `data.ai` — enabled-but-unhealthy is shown
    /// loudly because passthrough means listings arrive unfiltered.
    pub ai: Option<crate::tui::AiStatus>,
    last_spin: Instant,
}

impl Default for StatusBarState {
    fn default() -> Self {
        Self {
            daemon_up: false,
            new_count: 0,
            active_polls: Vec::new(),
            polling: Vec::new(),
            source_states: Vec::new(),
            last_poll_iso: None,
            poll_interval_sec: 0,
            spinner_tick: 0,
            bot_blocks: Vec::new(),
            ai: None,
            last_spin: Instant::now(),
        }
    }
}

impl StatusBarState {
    /// Advance spinner if active polls are running and enough time has passed.
    pub fn tick(&mut self) {
        if !self.active_polls.is_empty() && self.last_spin.elapsed().as_millis() >= 100 {
            self.spinner_tick = self.spinner_tick.wrapping_add(1);
            self.last_spin = Instant::now();
        }
    }
}

pub struct StatusBarWidget<'a> {
    state: &'a StatusBarState,
}

impl<'a> StatusBarWidget<'a> {
    pub fn new(state: &'a StatusBarState) -> Self {
        Self { state }
    }

    fn left_spans(&self) -> Vec<Span<'a>> {
        let mut spans: Vec<Span<'a>> = Vec::new();
        let dot = || Span::styled(" · ", Style::default().fg(colors::TEXT_DARK));

        // Daemon indicator
        if self.state.daemon_up {
            spans.push(Span::styled(" ●", Style::default().fg(colors::GREEN)));
        } else {
            spans.push(Span::styled(" ● down", Style::default().fg(colors::PINK)));
        }

        // AI evaluation health — a broken model must never fail silently:
        // listings still flow (passthrough), so this is the operator's only
        // in-app signal that filtering is off.
        match &self.state.ai {
            Some(ai) if ai.enabled && !ai.healthy => {
                spans.push(dot());
                // Truncate by chars, not bytes — ai.detail is a free-form
                // error string that may hold multibyte characters, and a
                // byte-index truncate would panic inside the render loop.
                let reason = if ai.detail.chars().count() > 44 {
                    let mut s: String = ai.detail.chars().take(43).collect();
                    s.push('…');
                    s
                } else {
                    ai.detail.clone()
                };
                spans.push(Span::styled(
                    format!("AI ✗ {reason}"),
                    Style::default().fg(colors::PINK).add_modifier(Modifier::BOLD),
                ));
            }
            Some(ai) if ai.enabled => {
                spans.push(dot());
                spans.push(Span::styled("AI ✓", Style::default().fg(colors::GREEN)));
            }
            Some(_) => {
                spans.push(dot());
                spans.push(Span::styled(
                    "AI off",
                    Style::default().fg(colors::TEXT_DIM),
                ));
            }
            None => {}
        }

        // Active poll spinner. With the sequential scheduler there is one
        // (profile, source) in flight at a time, so name the profile being
        // scraped — e.g. "⠙ polling  Lexus LC500 · eb". Falls back to the
        // bare source abbreviations if the daemon didn't send `polling`.
        if !self.state.polling.is_empty() {
            spans.push(dot());
            let ch = SPINNER[self.state.spinner_tick % SPINNER.len()];
            spans.push(Span::styled(ch.to_string(), Style::default().fg(colors::ORANGE)));
            spans.push(Span::styled(" polling ", Style::default().fg(colors::TEXT_DIM)));
            for (i, (profile, src)) in self.state.polling.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(", ", Style::default().fg(colors::TEXT_DIM)));
                }
                spans.push(Span::styled(
                    profile.clone(),
                    Style::default().fg(colors::TEXT_PRIMARY).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(" · ", Style::default().fg(colors::TEXT_DARK)));
                spans.push(Span::styled(
                    src.chars().take(2).collect::<String>(),
                    Style::default().fg(colors::src_color(src)).add_modifier(Modifier::BOLD),
                ));
            }
        } else if !self.state.active_polls.is_empty() {
            spans.push(dot());
            let ch = SPINNER[self.state.spinner_tick % SPINNER.len()];
            spans.push(Span::styled(ch.to_string(), Style::default().fg(colors::ORANGE)));
            for src in &self.state.active_polls {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    src.chars().take(2).collect::<String>(),
                    Style::default().fg(colors::src_color(src)).add_modifier(Modifier::BOLD),
                ));
            }
        }

        // Source states (idle sources)
        let idle_sources: Vec<&SourceState> = self
            .state
            .source_states
            .iter()
            .filter(|s| !self.state.active_polls.contains(&s.plugin_id))
            .collect();
        if !idle_sources.is_empty() {
            spans.push(dot());
            for (i, s) in idle_sources.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                let abbrev: String = s.plugin_id.chars().take(2).collect();
                spans.push(Span::styled(
                    abbrev,
                    Style::default().fg(colors::src_color(&s.plugin_id)),
                ));
                if s.consecutive_errors > 0 {
                    spans.push(Span::styled(
                        format!("!{}", s.consecutive_errors),
                        Style::default().fg(colors::PINK),
                    ));
                } else if let Some(ref last) = s.last_polled {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(last) {
                        let ago = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc))
                            .num_seconds();
                        spans.push(Span::styled(
                            format!(" {}", format_age(ago)),
                            Style::default().fg(colors::TEXT_DARK),
                        ));
                    }
                }
            }
        }

        // New count
        if self.state.new_count > 0 {
            spans.push(dot());
            spans.push(Span::styled(
                format!("{}", self.state.new_count),
                Style::default()
                    .fg(colors::BLUE)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(" new", Style::default().fg(colors::TEXT_DIM)));
        }

        // Last poll time + next poll countdown
        if let Some(ref iso) = self.state.last_poll_iso {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(iso) {
                let ago = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc)).num_seconds();
                spans.push(dot());
                spans.push(Span::styled(
                    format!("last {} ago", format_age(ago)),
                    Style::default().fg(colors::TEXT_DARK),
                ));
                if self.state.poll_interval_sec > 0 && self.state.active_polls.is_empty() {
                    let remaining = (self.state.poll_interval_sec as i64) - ago;
                    if remaining > 0 {
                        spans.push(Span::styled(
                            format!(" next {}", format_age(remaining)),
                            Style::default().fg(colors::TEXT_DARK),
                        ));
                    } else {
                        spans.push(Span::styled(" due", Style::default().fg(colors::ORANGE)));
                    }
                }
            }
        }

        // Bot-block indicator — prominent while any source is blocked, since
        // clearing it needs a manual visit to a visible browser tab ('x').
        if !self.state.bot_blocks.is_empty() {
            spans.push(dot());
            spans.push(Span::styled(
                format!(" \u{26a0} {} blocked", self.state.bot_blocks.len()),
                Style::default().fg(colors::PINK).add_modifier(Modifier::BOLD),
            ));
        }

        spans
    }

    fn right_spans(&self) -> Vec<Span<'a>> {
        let hotkey = |k: &'a str| Span::styled(k, Style::default().fg(colors::ORANGE));
        let suffix = |s: &'a str| Span::styled(s, Style::default().fg(colors::TEXT_DIMMER));
        let mut spans = vec![
            hotkey("a"),
            suffix("dd "),
            hotkey("e"),
            suffix("dit "),
            hotkey("r"),
            suffix("epoll "),
            hotkey("S"),
            suffix("ort "),
        ];
        if !self.state.bot_blocks.is_empty() {
            spans.push(hotkey("x"));
            spans.push(suffix("open-blocked "));
        }
        spans.push(hotkey("?"));
        spans.push(suffix("help "));
        spans.push(hotkey("q"));
        spans.push(suffix("uit"));
        spans
    }
}

impl Widget for StatusBarWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Fill background
        for x in area.left()..area.right() {
            for y in area.top()..area.bottom() {
                buf[(x, y)].set_bg(colors::BG_STATUS_BAR);
            }
        }

        let left = Line::from(self.left_spans());
        let right = Line::from(self.right_spans());

        // Render right-aligned hints
        let right_width = right.width() as u16;
        if area.width > right_width + 2 {
            let right_area = Rect {
                x: area.right().saturating_sub(right_width + 1),
                y: area.y,
                width: right_width + 1,
                height: 1,
            };
            buf.set_line(right_area.x, right_area.y, &right, right_area.width);
        }

        // Render left content
        let left_max = area.width.saturating_sub(right_width + 3);
        let left_area = Rect {
            x: area.x,
            y: area.y,
            width: left_max,
            height: 1,
        };
        buf.set_line(left_area.x, left_area.y, &left, left_area.width);
    }
}
