use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

use crate::tui::colors;

const MAX_LINES: usize = 500;

pub struct LogPanelState {
    log_path: PathBuf,
    lines: VecDeque<String>,
    last_size: u64,
    /// Lines scrolled back from the tail (0 = pinned to the live tail).
    /// Driven by the mouse wheel over the log panel.
    scroll_offset: usize,
}

impl LogPanelState {
    pub fn new(log_path: PathBuf) -> Self {
        let mut state = Self {
            log_path,
            lines: VecDeque::with_capacity(MAX_LINES),
            last_size: 0,
            scroll_offset: 0,
        };
        state.load_initial();
        state
    }

    fn load_initial(&mut self) {
        if let Ok(mut f) = File::open(&self.log_path) {
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_ok() {
                self.last_size = buf.len() as u64;
                for line in buf.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev() {
                    self.push_line(line.to_string());
                }
            }
        }
    }

    /// Poll for new data appended to the log file. Call this periodically.
    pub fn poll(&mut self) {
        let Ok(mut f) = File::open(&self.log_path) else {
            return;
        };
        let Ok(meta) = f.metadata() else {
            return;
        };
        let sz = meta.len();
        if sz <= self.last_size {
            if sz < self.last_size {
                // File was truncated/rotated
                self.last_size = 0;
            }
            return;
        }
        if f.seek(SeekFrom::Start(self.last_size)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_ok() {
            self.last_size = sz;
            let text = String::from_utf8_lossy(&buf);
            for line in text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    self.push_line(trimmed.to_string());
                }
            }
        }
    }

    fn push_line(&mut self, line: String) {
        if self.lines.len() >= MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(strip_ansi(&line));
    }

    pub fn lines(&self) -> &VecDeque<String> {
        &self.lines
    }

    /// Scroll back by `amount` lines, toward the start of the log.
    pub fn scroll_up(&mut self, amount: usize) {
        let max = self.lines.len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + amount).min(max);
    }

    /// Scroll forward by `amount` lines, toward the live tail.
    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }
}

/// Remove ANSI escape sequences (CSI color codes etc.). The daemon now
/// writes plain logs, but existing files — and anything else that appends
/// here — may carry escapes, and ratatui renders them as garble: the ESC
/// bytes break column math so lines overlap and `2m`/`0m` fragments leak.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // CSI: ESC [ <params 0x30-0x3F> <intermediates 0x20-0x2F> <final 0x40-0x7E>
        if chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    break;
                }
            }
        } else {
            // Two-char escape (ESC + single byte)
            chars.next();
        }
    }
    out
}

fn colorize_line(line: &str) -> Line<'_> {
    // Skip noise
    if line.contains("DeprecationWarning") || line.contains("node --trace") {
        return Line::from("");
    }

    if line.contains(" ERROR ") {
        return Line::from(Span::styled(
            line,
            Style::default()
                .fg(colors::PINK)
                .add_modifier(Modifier::BOLD),
        ));
    }

    if line.contains(" WARNING ") || line.contains(" WARN ") {
        if line.to_lowercase().contains("bot") || line.contains("Bot block") {
            return Line::from(Span::styled(
                format!(" \u{25b8} {line}"),
                Style::default()
                    .fg(colors::PINK)
                    .bg(colors::BOT_BLOCK_BG)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        return Line::from(Span::styled(line, Style::default().fg(colors::ORANGE)));
    }

    if line.contains(" INFO ") {
        if line.contains("Escalating") {
            return Line::from(Span::styled(
                format!("  \u{2605} {line}"),
                Style::default().fg(colors::PINK),
            ));
        }
        if line.contains("found") && line.contains("listings") {
            return Line::from(Span::styled(line, Style::default().fg(colors::GREEN)));
        }
        if line.contains("filter call:") || line.contains("frontier call:") {
            return Line::from(Span::styled(line, Style::default().fg(colors::BLUE)));
        }
        if line.contains("filter response:") || line.contains("frontier response:") {
            return Line::from(Span::styled(
                line,
                Style::default()
                    .fg(colors::BLUE)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if line.contains("Loaded new profile") || line.contains("Reload:") {
            return Line::from(Span::styled(
                line,
                Style::default()
                    .fg(colors::GREEN)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if line.contains("Daemon started") || line.contains("Connected to Chrome") {
            return Line::from(Span::styled(line, Style::default().fg(colors::TEXT_PRIMARY)));
        }
        if line.contains("executed successfully") {
            return Line::from(Span::styled(line, Style::default().fg(colors::TEXT_DARK)));
        }
        return Line::from(Span::styled(line, Style::default().fg(colors::TEXT_DIM)));
    }

    Line::from(Span::styled(line, Style::default().fg(colors::TEXT_DARK)))
}

pub struct LogPanelWidget<'a> {
    state: &'a LogPanelState,
    focused: bool,
}

impl<'a> LogPanelWidget<'a> {
    pub fn new(state: &'a LogPanelState, focused: bool) -> Self {
        Self { state, focused }
    }
}

impl Widget for LogPanelWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.focused {
            Style::default().fg(colors::ORANGE)
        } else {
            Style::default().fg(colors::INDICATOR_ACTIVE)
        };

        let block = Block::default()
            .title(Span::styled(
                " LOG",
                Style::default()
                    .fg(colors::TEXT_DIM)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::TOP)
            .border_style(border_style)
            .style(Style::default().bg(colors::BG_LOG));

        let inner = block.inner(area);
        block.render(area, buf);

        // Render the tail of the log, scrolled back by `scroll_offset` lines.
        let visible = inner.height as usize;
        let total = self.state.lines().len();
        let skip_from_end = self.state.scroll_offset.min(total);
        let lines: Vec<Line> = self
            .state
            .lines()
            .iter()
            .rev()
            .skip(skip_from_end)
            .take(visible)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|l| colorize_line(l))
            .collect();

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        paragraph.render(inner, buf);
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    #[test]
    fn scroll_up_bounded_by_line_count() {
        let mut state = LogPanelState {
            log_path: PathBuf::from("/nonexistent"),
            lines: (0..5).map(|i| i.to_string()).collect(),
            last_size: 0,
            scroll_offset: 0,
        };
        state.scroll_up(2);
        assert_eq!(state.scroll_offset, 2);
        state.scroll_up(100);
        assert_eq!(state.scroll_offset, 4); // len - 1
    }

    #[test]
    fn scroll_down_bounded_at_zero() {
        let mut state = LogPanelState {
            log_path: PathBuf::from("/nonexistent"),
            lines: (0..5).map(|i| i.to_string()).collect(),
            last_size: 0,
            scroll_offset: 3,
        };
        state.scroll_down(1);
        assert_eq!(state.scroll_offset, 2);
        state.scroll_down(100);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn strip_ansi_cleans_tracing_output() {
        // Verbatim shape of a tracing fmt line with ANSI enabled.
        let dirty = "\u{1b}[2m2026-08-08T20:51:52.029457Z\u{1b}[0m \u{1b}[33m WARN\u{1b}[0m bot block detected \u{1b}[3mplugin\u{1b}[0m\u{1b}[2m=\u{1b}[0mebay";
        assert_eq!(
            strip_ansi(dirty),
            "2026-08-08T20:51:52.029457Z  WARN bot block detected plugin=ebay"
        );
        // Plain lines pass through untouched.
        let clean = "2026-08-08 INFO Craigslist sfbay: found 25 listings";
        assert_eq!(strip_ansi(clean), clean);
    }

    #[test]
    fn push_line_strips_ansi_on_ingest() {
        let mut state = LogPanelState::new(std::path::PathBuf::from("/nonexistent"));
        state.push_line("\u{1b}[32m INFO\u{1b}[0m daemon started".to_string());
        assert_eq!(state.lines().back().unwrap(), " INFO daemon started");
    }
}
