use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::db::SourceState;

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

const CLR_BG: Color = Color::Rgb(0x11, 0x11, 0x11);
const CLR_ACCENT: Color = Color::Rgb(0xfd, 0x97, 0x1f);
const CLR_DIM: Color = Color::Rgb(0x55, 0x55, 0x55);
const CLR_MUTED: Color = Color::Rgb(0x75, 0x71, 0x5e);
const CLR_DATA: Color = Color::Rgb(0x66, 0xd9, 0xef);
const CLR_GREEN: Color = Color::Rgb(0xa6, 0xe2, 0x2e);
const CLR_RED: Color = Color::Rgb(0xf9, 0x26, 0x72);
const CLR_DARK: Color = Color::Rgb(0x3a, 0x3a, 0x3a);

const SRC_CLR_EBAY: Color = Color::Rgb(0xe6, 0xdb, 0x74);
const SRC_CLR_CRAIGSLIST: Color = CLR_RED;
const SRC_CLR_FACEBOOK: Color = CLR_DATA;

fn source_color(name: &str) -> Color {
    match name {
        "ebay" => SRC_CLR_EBAY,
        "craigslist" => SRC_CLR_CRAIGSLIST,
        "facebook" => SRC_CLR_FACEBOOK,
        _ => CLR_MUTED,
    }
}

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
    pub source_states: Vec<SourceState>,
    pub last_poll_iso: Option<String>,
    pub poll_interval_sec: u32,
    pub spinner_tick: usize,
    last_spin: Instant,
}

impl Default for StatusBarState {
    fn default() -> Self {
        Self {
            daemon_up: false,
            new_count: 0,
            active_polls: Vec::new(),
            source_states: Vec::new(),
            last_poll_iso: None,
            poll_interval_sec: 0,
            spinner_tick: 0,
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
        let dot = || Span::styled(" · ", Style::default().fg(CLR_DARK));

        // Daemon indicator
        if self.state.daemon_up {
            spans.push(Span::styled(" ●", Style::default().fg(CLR_GREEN)));
        } else {
            spans.push(Span::styled(" ● down", Style::default().fg(CLR_RED)));
        }

        // Active poll spinner
        if !self.state.active_polls.is_empty() {
            spans.push(dot());
            let ch = SPINNER[self.state.spinner_tick % SPINNER.len()];
            spans.push(Span::styled(
                ch.to_string(),
                Style::default().fg(CLR_ACCENT),
            ));
            for src in &self.state.active_polls {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    src[..2.min(src.len())].to_string(),
                    Style::default()
                        .fg(source_color(src))
                        .add_modifier(Modifier::BOLD),
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
                let abbrev = s.plugin_id[..2.min(s.plugin_id.len())].to_string();
                spans.push(Span::styled(
                    abbrev,
                    Style::default().fg(source_color(&s.plugin_id)),
                ));
                if s.consecutive_errors > 0 {
                    spans.push(Span::styled(
                        format!("!{}", s.consecutive_errors),
                        Style::default().fg(CLR_RED),
                    ));
                } else if let Some(ref last) = s.last_polled {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(last) {
                        let ago = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc))
                            .num_seconds();
                        spans.push(Span::styled(
                            format!(" {}", format_age(ago)),
                            Style::default().fg(CLR_DARK),
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
                    .fg(CLR_DATA)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(" new", Style::default().fg(CLR_MUTED)));
        }

        // Last poll time + next poll countdown
        if let Some(ref iso) = self.state.last_poll_iso {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(iso) {
                let ago = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc)).num_seconds();
                spans.push(dot());
                spans.push(Span::styled(
                    format!("last {} ago", format_age(ago)),
                    Style::default().fg(CLR_DARK),
                ));
                if self.state.poll_interval_sec > 0 && self.state.active_polls.is_empty() {
                    let remaining = (self.state.poll_interval_sec as i64) - ago;
                    if remaining > 0 {
                        spans.push(Span::styled(
                            format!(" next {}", format_age(remaining)),
                            Style::default().fg(CLR_DARK),
                        ));
                    } else {
                        spans.push(Span::styled(" due", Style::default().fg(CLR_ACCENT)));
                    }
                }
            }
        }

        spans
    }

    fn right_spans(&self) -> Vec<Span<'a>> {
        let hotkey = |k: &'a str| Span::styled(k, Style::default().fg(CLR_ACCENT));
        let suffix = |s: &'a str| Span::styled(s, Style::default().fg(CLR_DIM));
        vec![
            hotkey("a"),
            suffix("dd "),
            hotkey("e"),
            suffix("dit "),
            hotkey("r"),
            suffix("epoll "),
            hotkey("?"),
            suffix("help "),
            hotkey("q"),
            suffix("uit"),
        ]
    }
}

impl Widget for StatusBarWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Fill background
        for x in area.left()..area.right() {
            for y in area.top()..area.bottom() {
                buf[(x, y)].set_bg(CLR_BG);
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
