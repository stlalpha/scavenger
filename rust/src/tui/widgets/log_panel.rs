use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

const MAX_LINES: usize = 500;
const CLR_BG: Color = Color::Rgb(0x18, 0x18, 0x18);
const CLR_HDR: Color = Color::Rgb(0x75, 0x71, 0x5e);
const CLR_ERROR: Color = Color::Rgb(0xf9, 0x26, 0x72);
const CLR_WARN: Color = Color::Rgb(0xfd, 0x97, 0x1f);
const CLR_INFO: Color = Color::Rgb(0x75, 0x71, 0x5e);
const CLR_DIM: Color = Color::Rgb(0x3a, 0x3a, 0x3a);
const CLR_DATA: Color = Color::Rgb(0x66, 0xd9, 0xef);
const CLR_GREEN: Color = Color::Rgb(0xa6, 0xe2, 0x2e);
const CLR_FG: Color = Color::Rgb(0xf8, 0xf8, 0xf2);
const CLR_BOT_BG: Color = Color::Rgb(0x3a, 0x1a, 0x1a);

pub struct LogPanelState {
    log_path: PathBuf,
    lines: VecDeque<String>,
    last_size: u64,
}

impl LogPanelState {
    pub fn new(log_path: PathBuf) -> Self {
        let mut state = Self {
            log_path,
            lines: VecDeque::with_capacity(MAX_LINES),
            last_size: 0,
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
        self.lines.push_back(line);
    }

    pub fn lines(&self) -> &VecDeque<String> {
        &self.lines
    }
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
                .fg(CLR_ERROR)
                .add_modifier(Modifier::BOLD),
        ));
    }

    if line.contains(" WARNING ") {
        if line.to_lowercase().contains("bot") || line.contains("Bot block") {
            return Line::from(Span::styled(
                format!(" \u{25b8} {line}"),
                Style::default()
                    .fg(CLR_ERROR)
                    .bg(CLR_BOT_BG)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        return Line::from(Span::styled(line, Style::default().fg(CLR_WARN)));
    }

    if line.contains(" INFO ") {
        if line.contains("Escalating") {
            return Line::from(Span::styled(
                format!("  \u{2605} {line}"),
                Style::default().fg(CLR_ERROR),
            ));
        }
        if line.contains("found") && line.contains("listings") {
            return Line::from(Span::styled(line, Style::default().fg(CLR_GREEN)));
        }
        if line.contains("filter call:") || line.contains("frontier call:") {
            return Line::from(Span::styled(line, Style::default().fg(CLR_DATA)));
        }
        if line.contains("filter response:") || line.contains("frontier response:") {
            return Line::from(Span::styled(
                line,
                Style::default()
                    .fg(CLR_DATA)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if line.contains("Loaded new profile") || line.contains("Reload:") {
            return Line::from(Span::styled(
                line,
                Style::default()
                    .fg(CLR_GREEN)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if line.contains("Daemon started") || line.contains("Connected to Chrome") {
            return Line::from(Span::styled(line, Style::default().fg(CLR_FG)));
        }
        if line.contains("executed successfully") {
            return Line::from(Span::styled(line, Style::default().fg(CLR_DIM)));
        }
        return Line::from(Span::styled(line, Style::default().fg(CLR_INFO)));
    }

    Line::from(Span::styled(line, Style::default().fg(CLR_DIM)))
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
            Style::default().fg(Color::Rgb(0xfd, 0x97, 0x1f))
        } else {
            Style::default().fg(Color::Rgb(0x22, 0x22, 0x22))
        };

        let block = Block::default()
            .title(Span::styled(
                " LOG",
                Style::default()
                    .fg(CLR_HDR)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::TOP)
            .border_style(border_style)
            .style(Style::default().bg(CLR_BG));

        let inner = block.inner(area);
        block.render(area, buf);

        // Render visible tail of the log
        let visible = inner.height as usize;
        let lines: Vec<Line> = self
            .state
            .lines()
            .iter()
            .rev()
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
