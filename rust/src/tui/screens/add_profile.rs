use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};
use std::sync::LazyLock;

use regex::Regex;

use crate::models::Profile;
use crate::tui::colors;

static ID_SANITIZE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());

const POLL_OPTIONS: &[(&str, u64)] = &[
    ("5 min", 300),
    ("15 min", 900),
    ("30 min", 1800),
    ("1 hour", 3600),
    ("2 hours", 7200),
];

const PRIORITY_OPTIONS: &[&str] = &["high", "normal", "low"];

/// Convert keyword groups to display string.
pub fn keywords_to_string(keywords: &[crate::models::KeywordGroup]) -> String {
    use crate::models::KeywordGroup;
    keywords
        .iter()
        .map(|kg| match kg {
            KeywordGroup::Single(s) => s.clone(),
            KeywordGroup::Any(v) => v.join("|"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Parse user keyword input into KeywordGroup list.
pub fn parse_keywords(input: &str) -> Vec<crate::models::KeywordGroup> {
    use crate::models::KeywordGroup;
    input
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            if part.contains('|') {
                let variants: Vec<String> = part
                    .split('|')
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                    .collect();
                if variants.is_empty() {
                    None
                } else {
                    Some(KeywordGroup::Any(variants))
                }
            } else {
                Some(KeywordGroup::Single(part.to_string()))
            }
        })
        .collect()
}

/// Generate a profile id from a name: lowercase, replace non-alphanumeric runs with hyphens.
pub fn id_from_name(name: &str) -> String {
    let lower = name.to_lowercase();
    ID_SANITIZE_RE.replace_all(&lower, "-").trim_matches('-').to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Name,
    Keywords,
    NegativeKeywords,
    SourceEbay,
    SourceFacebook,
    SourceCraigslist,
    PriceMin,
    PriceMax,
    PollInterval,
    Priority,
    Tags,
    EscalationKeywords,
    LocationRadius,
}

impl FormField {
    pub const ALL: &[FormField] = &[
        FormField::Name,
        FormField::Keywords,
        FormField::NegativeKeywords,
        FormField::SourceEbay,
        FormField::SourceFacebook,
        FormField::SourceCraigslist,
        FormField::PriceMin,
        FormField::PriceMax,
        FormField::PollInterval,
        FormField::Priority,
        FormField::Tags,
        FormField::EscalationKeywords,
        FormField::LocationRadius,
    ];

    pub fn next(self) -> FormField {
        let idx = FormField::ALL.iter().position(|&f| f == self).unwrap_or(0);
        FormField::ALL[(idx + 1) % FormField::ALL.len()]
    }

    pub fn prev(self) -> FormField {
        let idx = FormField::ALL.iter().position(|&f| f == self).unwrap_or(0);
        FormField::ALL[(idx + FormField::ALL.len() - 1) % FormField::ALL.len()]
    }
}

/// Result of submitting the profile form.
#[derive(Debug, Clone)]
pub enum FormResult {
    Create(ProfileFormData),
    Update(ProfileFormData),
    Delete { id: String },
    Cancel,
}

#[derive(Debug, Clone)]
pub struct ProfileFormData {
    pub id: String,
    pub name: String,
    pub keywords: Vec<crate::models::KeywordGroup>,
    pub sources: Vec<String>,
    pub negative_keywords: Vec<String>,
    pub price_min: Option<f64>,
    pub price_max: Option<f64>,
    pub poll_interval_sec: u64,
    pub alert_priority: String,
    pub tags: Vec<String>,
    pub escalation_keywords: Vec<String>,
    pub location_radius_mi: Option<u32>,
}

/// Modal form for creating or editing a profile.
pub struct ProfileForm {
    editing: Option<Profile>,
    focused: FormField,
    // Text field values
    pub name: String,
    pub keywords: String,
    pub negative_keywords: String,
    pub src_ebay: bool,
    pub src_facebook: bool,
    pub src_craigslist: bool,
    pub price_min: String,
    pub price_max: String,
    pub poll_interval_idx: usize,
    pub priority_idx: usize,
    pub tags: String,
    pub escalation_keywords: String,
    pub location_radius: String,
    /// Cursor position within current text field.
    cursor_pos: usize,
    /// Whether a delete confirmation is being shown.
    pub confirm_delete: bool,
    /// Validation error to display.
    pub error_msg: Option<String>,
}

impl ProfileForm {
    pub fn new(profile: Option<Profile>) -> Self {
        let (name, keywords, negatives, src_ebay, src_facebook, src_craigslist, price_min, price_max, poll_idx, priority_idx, tags, escalation, radius) = match &profile {
            Some(p) => (
                p.name.clone(),
                keywords_to_string(&p.keywords),
                p.negative_keywords.join(", "),
                p.sources.contains(&"ebay".to_string()),
                p.sources.contains(&"facebook".to_string()),
                p.sources.contains(&"craigslist".to_string()),
                p.price_min.map(|v| format!("{}", v as i64)).unwrap_or_default(),
                p.price_max.map(|v| format!("{}", v as i64)).unwrap_or_default(),
                POLL_OPTIONS.iter().position(|&(_, v)| v == p.poll_interval_sec).unwrap_or(3),
                PRIORITY_OPTIONS.iter().position(|&v| v == p.alert_priority.to_string()).unwrap_or(1),
                p.tags.join(", "),
                p.escalation_keywords.join(", "),
                p.location_radius_mi.map(|v| v.to_string()).unwrap_or_default(),
            ),
            None => (
                String::new(),
                String::new(),
                String::new(),
                true,
                true,
                true,
                String::new(),
                String::new(),
                3, // 1 hour default
                1, // normal default
                String::new(),
                String::new(),
                String::new(),
            ),
        };

        let cursor_pos = name.len();
        Self {
            editing: profile,
            focused: FormField::Name,
            name,
            keywords,
            negative_keywords: negatives,
            src_ebay,
            src_facebook,
            src_craigslist,
            price_min,
            price_max,
            poll_interval_idx: poll_idx,
            priority_idx,
            tags,
            escalation_keywords: escalation,
            location_radius: radius,
            cursor_pos,
            confirm_delete: false,
            error_msg: None,
        }
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    pub fn focused_field(&self) -> FormField {
        self.focused
    }

    pub fn focus_next(&mut self) {
        self.focused = self.focused.next();
        self.cursor_pos = self.current_field_text().len();
    }

    pub fn focus_prev(&mut self) {
        self.focused = self.focused.prev();
        self.cursor_pos = self.current_field_text().len();
    }

    fn current_field_text(&self) -> &str {
        match self.focused {
            FormField::Name => &self.name,
            FormField::Keywords => &self.keywords,
            FormField::NegativeKeywords => &self.negative_keywords,
            FormField::PriceMin => &self.price_min,
            FormField::PriceMax => &self.price_max,
            FormField::Tags => &self.tags,
            FormField::EscalationKeywords => &self.escalation_keywords,
            FormField::LocationRadius => &self.location_radius,
            _ => "",
        }
    }

    /// Get mutable reference to the text field for the focused form element.
    /// Returns None for non-text fields (checkboxes, selects).
    fn focused_field_mut(&mut self) -> Option<&mut String> {
        match self.focused {
            FormField::Name => Some(&mut self.name),
            FormField::Keywords => Some(&mut self.keywords),
            FormField::NegativeKeywords => Some(&mut self.negative_keywords),
            FormField::PriceMin => Some(&mut self.price_min),
            FormField::PriceMax => Some(&mut self.price_max),
            FormField::Tags => Some(&mut self.tags),
            FormField::EscalationKeywords => Some(&mut self.escalation_keywords),
            FormField::LocationRadius => Some(&mut self.location_radius),
            _ => None,
        }
    }

    /// Handle a character input.
    pub fn type_char(&mut self, ch: char) {
        // Toggle checkboxes with space
        match self.focused {
            FormField::SourceEbay => {
                if ch == ' ' {
                    self.src_ebay = !self.src_ebay;
                }
                return;
            }
            FormField::SourceFacebook => {
                if ch == ' ' {
                    self.src_facebook = !self.src_facebook;
                }
                return;
            }
            FormField::SourceCraigslist => {
                if ch == ' ' {
                    self.src_craigslist = !self.src_craigslist;
                }
                return;
            }
            FormField::PollInterval => {
                if ch == ' ' || ch == '\t' {
                    self.poll_interval_idx = (self.poll_interval_idx + 1) % POLL_OPTIONS.len();
                }
                return;
            }
            FormField::Priority => {
                if ch == ' ' || ch == '\t' {
                    self.priority_idx = (self.priority_idx + 1) % PRIORITY_OPTIONS.len();
                }
                return;
            }
            _ => {}
        }

        let cursor_pos = self.cursor_pos;
        if let Some(field) = self.focused_field_mut() {
            let pos = cursor_pos.min(field.len());
            field.insert(pos, ch);
            self.cursor_pos = pos + ch.len_utf8();
            self.error_msg = None;
        }
    }

    /// Handle backspace.
    pub fn backspace(&mut self) {
        let cursor_pos = self.cursor_pos;
        if let Some(field) = self.focused_field_mut() {
            if cursor_pos > 0 && cursor_pos <= field.len() {
                let prev_boundary = field[..cursor_pos]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                field.remove(prev_boundary);
                self.cursor_pos = prev_boundary;
            }
        }
    }

    /// Move cursor left within current field.
    pub fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let text = self.current_field_text();
            self.cursor_pos = text[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right within current field.
    pub fn cursor_right(&mut self) {
        let len = self.current_field_text().len();
        if self.cursor_pos < len {
            let text = self.current_field_text();
            self.cursor_pos = text[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(len);
        }
    }

    /// Validate and build the form result.
    pub fn submit(&mut self) -> Option<FormResult> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            self.error_msg = Some("Name is required".into());
            return None;
        }

        let keywords = parse_keywords(&self.keywords);
        if keywords.is_empty() {
            self.error_msg = Some("At least one keyword is required".into());
            return None;
        }

        let mut sources = Vec::new();
        if self.src_ebay {
            sources.push("ebay".to_string());
        }
        if self.src_facebook {
            sources.push("facebook".to_string());
        }
        if self.src_craigslist {
            sources.push("craigslist".to_string());
        }
        if sources.is_empty() {
            self.error_msg = Some("Select at least one source".into());
            return None;
        }

        let price_min = if self.price_min.trim().is_empty() {
            None
        } else {
            match self.price_min.trim().parse::<f64>() {
                Ok(v) => Some(v),
                Err(_) => {
                    self.error_msg = Some("Invalid minimum price".into());
                    return None;
                }
            }
        };

        let price_max = if self.price_max.trim().is_empty() {
            None
        } else {
            match self.price_max.trim().parse::<f64>() {
                Ok(v) => Some(v),
                Err(_) => {
                    self.error_msg = Some("Invalid maximum price".into());
                    return None;
                }
            }
        };

        let radius = if self.location_radius.trim().is_empty() {
            None
        } else {
            match self.location_radius.trim().parse::<u32>() {
                Ok(v) => Some(v),
                Err(_) => {
                    self.error_msg = Some("Invalid radius".into());
                    return None;
                }
            }
        };

        let negatives: Vec<String> = self
            .negative_keywords
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let tags: Vec<String> = self
            .tags
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let escalation: Vec<String> = self
            .escalation_keywords
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let poll_interval_sec = POLL_OPTIONS[self.poll_interval_idx].1;
        let alert_priority = PRIORITY_OPTIONS[self.priority_idx].to_string();

        let (id, action_fn): (String, fn(ProfileFormData) -> FormResult) = if let Some(ref p) = self.editing {
            (p.id.clone(), FormResult::Update)
        } else {
            (id_from_name(&name), FormResult::Create)
        };

        let data = ProfileFormData {
            id,
            name,
            keywords,
            sources,
            negative_keywords: negatives,
            price_min,
            price_max,
            poll_interval_sec,
            alert_priority,
            tags,
            escalation_keywords: escalation,
            location_radius_mi: radius,
        };

        Some(action_fn(data))
    }

    /// Request delete (shows confirmation, then returns FormResult::Delete).
    pub fn request_delete(&mut self) -> Option<FormResult> {
        self.editing.as_ref()?;
        if !self.confirm_delete {
            self.confirm_delete = true;
            return None;
        }
        let id = self.editing.as_ref().unwrap().id.clone();
        Some(FormResult::Delete { id })
    }

    pub fn cancel_delete(&mut self) {
        self.confirm_delete = false;
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        // Dialog dimensions: centered, 64 wide, up to 80% height
        let dialog_width = 64u16.min(area.width.saturating_sub(4));
        let dialog_height = (area.height * 4 / 5).min(area.height.saturating_sub(4));

        let [dialog_area] = Layout::horizontal([Constraint::Length(dialog_width)])
            .flex(Flex::Center)
            .areas(area);
        let [dialog_area] = Layout::vertical([Constraint::Length(dialog_height)])
            .flex(Flex::Center)
            .areas(dialog_area);

        // Clear background
        Clear.render(dialog_area, buf);

        let title = if let Some(ref p) = self.editing {
            format!("EDIT \u{2014} {}", p.name)
        } else {
            "NEW PROFILE".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BLUE))
            .title(Span::styled(
                format!(" {} ", title),
                Style::default()
                    .fg(colors::BLUE)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center)
            .style(Style::default().bg(colors::BG_DETAIL));

        let inner = block.inner(dialog_area);
        Widget::render(block, dialog_area, buf);

        if inner.width < 4 || inner.height < 4 {
            return;
        }

        // Field groups, each an indivisible unit of rows (label/hint/input/
        // spacer). Rendering only ever draws a group whole, never split
        // across the visible/hidden boundary, so nothing can paint past
        // the dialog border and Tab always keeps the focused group intact.
        const GROUP_HEIGHTS: [u16; 9] = [3, 4, 3, 3, 3, 3, 3, 4, 3];

        fn group_for_field(field: FormField) -> usize {
            match field {
                FormField::Name => 0,
                FormField::Keywords => 1,
                FormField::NegativeKeywords => 2,
                FormField::SourceEbay | FormField::SourceFacebook | FormField::SourceCraigslist => 3,
                FormField::PriceMin | FormField::PriceMax => 4,
                FormField::PollInterval | FormField::Priority => 5,
                FormField::Tags => 6,
                FormField::EscalationKeywords => 7,
                FormField::LocationRadius => 8,
            }
        }

        let has_error = self.error_msg.is_some();
        let n_groups = GROUP_HEIGHTS.len();
        let mut offsets = vec![0u16; n_groups + 1];
        for i in 0..GROUP_HEIGHTS.len() {
            offsets[i + 1] = offsets[i] + GROUP_HEIGHTS[i];
        }

        // Reserve the bottom row for the hint bar, and a second one above it
        // for the error message when present, so both are always visible
        // independent of how much of the field stack fits above them.
        let body_height = inner.height.saturating_sub(if has_error { 2 } else { 1 });

        // Scroll just far enough to keep the focused field's whole group
        // on screen, preferring to show as much preceding content as fits.
        let focused_group = group_for_field(self.focused);
        let focused_start = offsets[focused_group];
        let focused_end = offsets[focused_group + 1];
        let mut scroll = focused_end.saturating_sub(body_height);
        if focused_start < scroll {
            scroll = focused_start;
        }

        let render_label = |area: Rect, buf: &mut Buffer, label: &str| {
            Paragraph::new(Span::styled(
                label,
                Style::default()
                    .fg(colors::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ))
            .render(area, buf);
        };

        let render_hint = |area: Rect, buf: &mut Buffer, hint: &str| {
            Paragraph::new(Span::styled(hint, Style::default().fg(colors::TEXT_DIM)))
                .render(area, buf);
        };

        let render_input = |area: Rect, buf: &mut Buffer, value: &str, focused: bool, cursor_pos: usize| {
            let style = if focused {
                Style::default().fg(colors::TEXT_PRIMARY).bg(colors::BG_SELECTED)
            } else {
                Style::default().fg(colors::TEXT_DIM).bg(colors::BG_SIDEBAR)
            };
            let block = Block::default()
                .borders(Borders::BOTTOM)
                .border_style(if focused {
                    Style::default().fg(colors::BLUE)
                } else {
                    Style::default().fg(colors::INDICATOR_ACTIVE)
                });
            let text_area = block.inner(area);
            Widget::render(block, area, buf);
            if text_area.width == 0 {
                return;
            }
            let (display_col, scroll) = caret_scroll(value, cursor_pos, text_area.width);
            Paragraph::new(value)
                .style(style)
                .scroll((0, scroll))
                .render(text_area, buf);
            if focused {
                let cx = text_area.x + (display_col - scroll).min(text_area.width - 1);
                if let Some(cell) = buf.cell_mut((cx, text_area.y)) {
                    cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
                }
            }
        };

        let render_checkbox = |area: Rect, buf: &mut Buffer, label: &str, checked: bool, focused: bool| {
            let indicator = if checked { "[x]" } else { "[ ]" };
            let style = if focused {
                Style::default().fg(colors::BLUE)
            } else {
                Style::default().fg(colors::TEXT_PRIMARY)
            };
            Paragraph::new(format!("{} {}", indicator, label))
                .style(style)
                .render(area, buf);
        };

        let render_select = |area: Rect, buf: &mut Buffer, options: &[&str], selected: usize, focused: bool| {
            let display = options.get(selected).copied().unwrap_or("?");
            let style = if focused {
                Style::default().fg(colors::BLUE).bg(colors::BG_SELECTED)
            } else {
                Style::default().fg(colors::TEXT_DIM).bg(colors::BG_SIDEBAR)
            };
            Paragraph::new(format!("< {} >", display))
                .style(style)
                .render(area, buf);
        };

        for group in 0..n_groups {
            let start = offsets[group];
            let end = offsets[group + 1];
            if start < scroll || end > scroll + body_height {
                continue;
            }
            let mut y = inner.y + (start - scroll);
            let field_area = |y: &mut u16, height: u16| -> Rect {
                let r = Rect::new(inner.x + 1, *y, inner.width.saturating_sub(2), height);
                *y += height;
                r
            };

            match group {
                0 => {
                    render_label(field_area(&mut y, 1), buf, "Name");
                    render_input(
                        field_area(&mut y, 2),
                        buf,
                        &self.name,
                        self.focused == FormField::Name,
                        self.cursor_pos,
                    );
                }
                1 => {
                    render_label(field_area(&mut y, 1), buf, "Keywords");
                    render_hint(field_area(&mut y, 1), buf, "comma-separated, | for OR groups");
                    render_input(
                        field_area(&mut y, 2),
                        buf,
                        &self.keywords,
                        self.focused == FormField::Keywords,
                        self.cursor_pos,
                    );
                }
                2 => {
                    render_label(field_area(&mut y, 1), buf, "Negative keywords");
                    render_input(
                        field_area(&mut y, 2),
                        buf,
                        &self.negative_keywords,
                        self.focused == FormField::NegativeKeywords,
                        self.cursor_pos,
                    );
                }
                3 => {
                    render_label(field_area(&mut y, 1), buf, "Sources");
                    let src_row = field_area(&mut y, 1);
                    let thirds = Layout::horizontal([
                        Constraint::Ratio(1, 3),
                        Constraint::Ratio(1, 3),
                        Constraint::Ratio(1, 3),
                    ])
                    .split(src_row);
                    render_checkbox(thirds[0], buf, "eBay", self.src_ebay, self.focused == FormField::SourceEbay);
                    render_checkbox(thirds[1], buf, "Facebook", self.src_facebook, self.focused == FormField::SourceFacebook);
                    render_checkbox(thirds[2], buf, "Craigslist", self.src_craigslist, self.focused == FormField::SourceCraigslist);
                }
                4 => {
                    render_label(field_area(&mut y, 1), buf, "Price range");
                    let price_row = field_area(&mut y, 2);
                    let [min_area, sep_area, max_area] = Layout::horizontal([
                        Constraint::Ratio(2, 5),
                        Constraint::Length(5),
                        Constraint::Ratio(2, 5),
                    ])
                    .areas(price_row);
                    render_input(min_area, buf, &self.price_min, self.focused == FormField::PriceMin, self.cursor_pos);
                    Paragraph::new(" to ")
                        .style(Style::default().fg(colors::TEXT_DIM))
                        .alignment(Alignment::Center)
                        .render(sep_area, buf);
                    render_input(max_area, buf, &self.price_max, self.focused == FormField::PriceMax, self.cursor_pos);
                }
                5 => {
                    let pair_row = field_area(&mut y, 1);
                    let [left, right] = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
                        .areas(pair_row);
                    Paragraph::new(Span::styled(
                        "Poll every",
                        Style::default().fg(colors::TEXT_PRIMARY).add_modifier(Modifier::BOLD),
                    )).render(left, buf);
                    Paragraph::new(Span::styled(
                        "Priority",
                        Style::default().fg(colors::TEXT_PRIMARY).add_modifier(Modifier::BOLD),
                    )).render(right, buf);

                    let select_row = field_area(&mut y, 1);
                    let [left, right] = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
                        .areas(select_row);
                    let poll_labels: Vec<&str> = POLL_OPTIONS.iter().map(|&(l, _)| l).collect();
                    render_select(left, buf, &poll_labels, self.poll_interval_idx, self.focused == FormField::PollInterval);
                    render_select(right, buf, PRIORITY_OPTIONS, self.priority_idx, self.focused == FormField::Priority);
                }
                6 => {
                    render_label(field_area(&mut y, 1), buf, "Tags");
                    render_input(
                        field_area(&mut y, 2),
                        buf,
                        &self.tags,
                        self.focused == FormField::Tags,
                        self.cursor_pos,
                    );
                }
                7 => {
                    render_label(field_area(&mut y, 1), buf, "Escalation keywords");
                    render_hint(field_area(&mut y, 1), buf, "trigger deeper AI eval");
                    render_input(
                        field_area(&mut y, 2),
                        buf,
                        &self.escalation_keywords,
                        self.focused == FormField::EscalationKeywords,
                        self.cursor_pos,
                    );
                }
                8 => {
                    render_label(field_area(&mut y, 1), buf, "Location radius (miles)");
                    render_input(
                        field_area(&mut y, 2),
                        buf,
                        &self.location_radius,
                        self.focused == FormField::LocationRadius,
                        self.cursor_pos,
                    );
                }
                _ => unreachable!(),
            }
        }

        // Error message — pinned above the hint bar, independent of scroll,
        // so a validation failure is always visible instead of landing past
        // the field stack's scroll window.
        if let Some(ref err) = self.error_msg {
            let err_y = inner.y + inner.height - 2;
            let err_area = Rect::new(inner.x + 1, err_y, inner.width.saturating_sub(2), 1);
            Paragraph::new(Span::styled(err.clone(), Style::default().fg(colors::PINK)))
                .render(err_area, buf);
        }

        // Delete confirmation — a fixed centered overlay independent of the
        // field stack above, so it's always visible regardless of terminal
        // size or scroll position (the field stack, by contrast, can hide
        // fields and would otherwise hide this too on short terminals).
        if self.confirm_delete {
            render_confirm_overlay(
                area,
                buf,
                &[
                    Line::from(Span::styled(
                        "Delete this profile and all its listings?",
                        Style::default().fg(colors::PINK),
                    )),
                    Line::from(vec![
                        Span::styled("y", Style::default().fg(colors::ORANGE)),
                        Span::styled("es", Style::default().fg(colors::TEXT_DIM)),
                        Span::raw("   "),
                        Span::styled("n", Style::default().fg(colors::ORANGE)),
                        Span::styled("o", Style::default().fg(colors::TEXT_DIM)),
                    ]),
                ],
            );
        }

        // Bottom buttons hint — pinned to the dialog's last inner row
        // regardless of how much of the field stack is visible above it.
        let bottom_y = inner.y + inner.height - 1;
        let btn_area = Rect::new(inner.x + 1, bottom_y, inner.width.saturating_sub(2), 1);
        let mut btn_spans = vec![
            Span::styled("Esc", Style::default().fg(colors::ORANGE)),
            Span::styled(" cancel", Style::default().fg(colors::TEXT_DIM)),
            Span::raw("  "),
            Span::styled("Enter", Style::default().fg(colors::ORANGE)),
            Span::styled(
                if self.is_editing() { " save" } else { " create" },
                Style::default().fg(colors::TEXT_DIM),
            ),
        ];
        if self.is_editing() {
            btn_spans.push(Span::raw("  "));
            btn_spans.push(Span::styled("Del", Style::default().fg(colors::PINK)));
            btn_spans.push(Span::styled(" delete", Style::default().fg(colors::TEXT_DIM)));
        }
        Paragraph::new(Line::from(btn_spans))
            .alignment(Alignment::Right)
            .render(btn_area, buf);
    }
}

/// Display column and horizontal scroll offset for a text input's caret.
/// `cursor_pos` is a byte offset into `value`; display column is
/// approximated as a char count (not full unicode-width) since this crate
/// doesn't carry `unicode-width` as a direct dependency — good enough to
/// keep the caret roughly aligned for the common case, exact for pure-ASCII
/// input.
fn caret_scroll(value: &str, cursor_pos: usize, width: u16) -> (u16, u16) {
    let display_col = value[..cursor_pos.min(value.len())].chars().count() as u16;
    let scroll = display_col.saturating_sub(width.saturating_sub(1));
    (display_col, scroll)
}

/// Small centered confirmation overlay, laid out independently of any
/// dialog's field stack so it can never be scrolled or clipped off-screen.
/// Shared by the profile-delete and daemon-quit confirmations.
pub fn render_confirm_overlay(area: Rect, buf: &mut Buffer, lines: &[Line<'static>]) {
    let width = 44u16.min(area.width.saturating_sub(2));
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    if width == 0 || height == 0 {
        return;
    }

    let [overlay] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [overlay] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(overlay);

    Clear.render(overlay, buf);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::PINK))
        .style(Style::default().bg(colors::BG_DETAIL));
    let inner = block.inner(overlay);
    Widget::render(block, overlay, buf);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    Paragraph::new(lines.to_vec())
        .alignment(Alignment::Center)
        .render(inner, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AlertPriority, KeywordGroup};

    #[test]
    fn parse_keywords_single_terms() {
        let groups = parse_keywords("bike, helmet");
        assert_eq!(
            groups,
            vec![
                KeywordGroup::Single("bike".into()),
                KeywordGroup::Single("helmet".into()),
            ]
        );
    }

    #[test]
    fn parse_keywords_or_group_syntax() {
        let groups = parse_keywords("road bike|gravel bike, helmet");
        assert_eq!(
            groups,
            vec![
                KeywordGroup::Any(vec!["road bike".into(), "gravel bike".into()]),
                KeywordGroup::Single("helmet".into()),
            ]
        );
    }

    #[test]
    fn parse_keywords_trims_whitespace_and_drops_empty_entries() {
        let groups = parse_keywords(" bike ,, helmet|pads ");
        assert_eq!(
            groups,
            vec![
                KeywordGroup::Single("bike".into()),
                KeywordGroup::Any(vec!["helmet".into(), "pads".into()]),
            ]
        );
    }

    #[test]
    fn parse_keywords_blank_input_yields_no_groups() {
        assert!(parse_keywords("").is_empty());
        assert!(parse_keywords("   ,  ,  ").is_empty());
    }

    #[test]
    fn keywords_to_string_round_trips_through_parse() {
        let original = "bike, road|gravel, helmet";
        let groups = parse_keywords(original);
        let rendered = keywords_to_string(&groups);
        assert_eq!(rendered, "bike, road|gravel, helmet");
        assert_eq!(parse_keywords(&rendered), groups);
    }

    #[test]
    fn id_from_name_lowercases_and_hyphenates() {
        assert_eq!(id_from_name("Mountain Bikes!"), "mountain-bikes");
        assert_eq!(id_from_name("  Vinyl -- Records  "), "vinyl-records");
    }

    fn filled_form() -> ProfileForm {
        let mut form = ProfileForm::new(None);
        form.name = "Mountain Bikes".into();
        form.keywords = "bike, road|gravel".into();
        form.src_ebay = true;
        form.src_facebook = false;
        form.src_craigslist = false;
        form
    }

    #[test]
    fn submit_requires_name() {
        let mut form = filled_form();
        form.name.clear();
        assert!(form.submit().is_none());
        assert_eq!(form.error_msg.as_deref(), Some("Name is required"));
    }

    #[test]
    fn submit_requires_keywords() {
        let mut form = filled_form();
        form.keywords.clear();
        assert!(form.submit().is_none());
        assert_eq!(
            form.error_msg.as_deref(),
            Some("At least one keyword is required")
        );
    }

    #[test]
    fn submit_requires_a_source() {
        let mut form = filled_form();
        form.src_ebay = false;
        assert!(form.submit().is_none());
        assert_eq!(form.error_msg.as_deref(), Some("Select at least one source"));
    }

    #[test]
    fn submit_rejects_non_numeric_price_min() {
        let mut form = filled_form();
        form.price_min = "cheap".into();
        assert!(form.submit().is_none());
        assert_eq!(form.error_msg.as_deref(), Some("Invalid minimum price"));
    }

    #[test]
    fn submit_rejects_non_numeric_price_max() {
        let mut form = filled_form();
        form.price_max = "lots".into();
        assert!(form.submit().is_none());
        assert_eq!(form.error_msg.as_deref(), Some("Invalid maximum price"));
    }

    #[test]
    fn submit_rejects_non_numeric_radius() {
        let mut form = filled_form();
        form.location_radius = "far".into();
        assert!(form.submit().is_none());
        assert_eq!(form.error_msg.as_deref(), Some("Invalid radius"));
    }

    #[test]
    fn submit_rejects_negative_radius() {
        let mut form = filled_form();
        form.location_radius = "-5".into();
        assert!(form.submit().is_none());
        assert_eq!(form.error_msg.as_deref(), Some("Invalid radius"));
    }

    #[test]
    fn submit_new_profile_yields_create_with_derived_id() {
        let mut form = filled_form();
        form.price_min = "50".into();
        form.price_max = "500".into();
        form.location_radius = "25".into();
        match form.submit() {
            Some(FormResult::Create(data)) => {
                assert_eq!(data.id, "mountain-bikes");
                assert_eq!(data.name, "Mountain Bikes");
                assert_eq!(data.sources, vec!["ebay".to_string()]);
                assert_eq!(data.price_min, Some(50.0));
                assert_eq!(data.price_max, Some(500.0));
                assert_eq!(data.location_radius_mi, Some(25));
                assert_eq!(
                    data.keywords,
                    vec![
                        KeywordGroup::Single("bike".into()),
                        KeywordGroup::Any(vec!["road".into(), "gravel".into()]),
                    ]
                );
            }
            other => panic!("expected Create, got {other:?}"),
        }
    }

    #[test]
    fn submit_editing_profile_yields_update_preserving_id() {
        let existing = Profile {
            id: "bikes".into(),
            name: "Old Name".into(),
            keywords: vec![KeywordGroup::Single("bike".into())],
            negative_keywords: vec![],
            sources: vec!["ebay".into()],
            price_min: None,
            price_max: None,
            poll_interval_sec: 3600,
            alert_priority: AlertPriority::Normal,
            enabled: true,
            tags: vec![],
            escalation_keywords: vec![],
            location_radius_mi: None,
        };
        let mut form = ProfileForm::new(Some(existing));
        form.name = "New Name".into();
        match form.submit() {
            Some(FormResult::Update(data)) => {
                assert_eq!(data.id, "bikes");
                assert_eq!(data.name, "New Name");
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn request_delete_requires_two_confirmations() {
        let existing = Profile {
            id: "bikes".into(),
            name: "Bikes".into(),
            keywords: vec![KeywordGroup::Single("bike".into())],
            negative_keywords: vec![],
            sources: vec!["ebay".into()],
            price_min: None,
            price_max: None,
            poll_interval_sec: 3600,
            alert_priority: AlertPriority::Normal,
            enabled: true,
            tags: vec![],
            escalation_keywords: vec![],
            location_radius_mi: None,
        };
        let mut form = ProfileForm::new(Some(existing));

        assert!(form.request_delete().is_none());
        assert!(form.confirm_delete);

        match form.request_delete() {
            Some(FormResult::Delete { id }) => assert_eq!(id, "bikes"),
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn request_delete_on_new_profile_is_a_noop() {
        let mut form = ProfileForm::new(None);
        assert!(form.request_delete().is_none());
        assert!(!form.confirm_delete);
    }

    #[test]
    fn cancel_delete_clears_confirmation_state() {
        let mut form = filled_form();
        form.confirm_delete = true;
        form.cancel_delete();
        assert!(!form.confirm_delete);
    }

    // -- caret_scroll: display-column and horizontal-scroll math (F3) --

    #[test]
    fn caret_scroll_stays_put_when_text_fits() {
        let (col, scroll) = caret_scroll("bike", 4, 20);
        assert_eq!(col, 4);
        assert_eq!(scroll, 0);
    }

    #[test]
    fn caret_scroll_advances_once_cursor_passes_the_visible_width() {
        let value = "a".repeat(30);
        let (col, scroll) = caret_scroll(&value, 30, 10);
        assert_eq!(col, 30);
        // Caret pinned to the last visible column (width - 1).
        assert_eq!(scroll, 30 - 9);
        assert_eq!(col - scroll, 9);
    }

    #[test]
    fn caret_scroll_tracks_cursor_in_the_middle_of_long_text() {
        let value = "a".repeat(30);
        // Cursor sitting well inside the string, past the visible window.
        let (col, scroll) = caret_scroll(&value, 20, 10);
        assert_eq!(col, 20);
        assert_eq!(scroll, 20 - 9);
    }

    #[test]
    fn caret_scroll_uses_char_count_not_byte_length_for_multibyte_text() {
        // "é" is 2 bytes in UTF-8 but one display column under this
        // approximation — a byte-offset caret would land one column short.
        let value = "éé";
        assert_eq!(value.len(), 4);
        let (col, scroll) = caret_scroll(value, value.len(), 20);
        assert_eq!(col, 2);
        assert_eq!(scroll, 0);
    }

    #[test]
    fn caret_scroll_at_cursor_zero_is_flush_left() {
        let (col, scroll) = caret_scroll("bike", 0, 20);
        assert_eq!(col, 0);
        assert_eq!(scroll, 0);
    }

    // -- render(): field-stack scrolling and overlay placement (F1/F2) --

    fn buffer_text(buf: &Buffer) -> String {
        let area = buf.area;
        let mut out = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                out.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
            }
            out.push('\n');
        }
        out
    }

    /// Below the ~29 rows the full field stack needs, the focused field
    /// must still render fully inside the dialog border, and the hint row
    /// must still be pinned at the bottom — never pushed off-screen or
    /// painted past the border.
    #[test]
    fn short_terminal_keeps_focused_field_and_hint_row_on_screen() {
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        let mut form = ProfileForm::new(None);
        // Focus the last field in the stack — on a short terminal this is
        // exactly the one a naive fixed layout would clip or overflow.
        for _ in 0..(FormField::ALL.len() - 1) {
            form.focus_next();
        }
        assert_eq!(form.focused_field(), FormField::LocationRadius);

        form.render(area, &mut buf);

        let text = buffer_text(&buf);
        assert!(
            text.contains("Location radius"),
            "focused field's label should be visible, got:\n{text}"
        );
        assert!(
            text.contains("Esc") && text.contains("cancel"),
            "hint row should always be pinned and visible, got:\n{text}"
        );
    }

    /// The delete confirmation must render even on a terminal far too
    /// short for the full field stack — it's a standalone overlay, not
    /// appended after the fields.
    #[test]
    fn delete_confirmation_renders_on_a_short_terminal() {
        let existing = Profile {
            id: "bikes".into(),
            name: "Bikes".into(),
            keywords: vec![KeywordGroup::Single("bike".into())],
            negative_keywords: vec![],
            sources: vec!["ebay".into()],
            price_min: None,
            price_max: None,
            poll_interval_sec: 3600,
            alert_priority: AlertPriority::Normal,
            enabled: true,
            tags: vec![],
            escalation_keywords: vec![],
            location_radius_mi: None,
        };
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        let mut form = ProfileForm::new(Some(existing));
        form.confirm_delete = true;

        form.render(area, &mut buf);

        let text = buffer_text(&buf);
        assert!(
            text.contains("Delete this profile"),
            "delete confirmation must always render, got:\n{text}"
        );
    }

    /// A long value scrolls so the caret stays visible instead of running
    /// off the edge of the input box.
    #[test]
    fn long_field_value_keeps_caret_within_the_dialog_border() {
        let area = Rect::new(0, 0, 80, 40);
        let mut buf = Buffer::empty(area);
        let mut form = ProfileForm::new(None);
        form.name = "a".repeat(120);
        form.type_char('!'); // moves cursor to the end, past any fixed width

        form.render(area, &mut buf);

        // The dialog is 64 cols wide with a 1-col border and 1-col margin
        // on each side, so the input can never legitimately need a column
        // outside the dialog's own bounds.
        let dialog_right = {
            let dialog_width = 64u16.min(area.width.saturating_sub(4));
            (area.width - dialog_width) / 2 + dialog_width
        };
        let mut found_reversed = false;
        for x in area.left()..area.right() {
            for y in area.top()..area.bottom() {
                if buf.cell((x, y)).is_some_and(|c| c.modifier.contains(Modifier::REVERSED)) {
                    found_reversed = true;
                    assert!(
                        x < dialog_right,
                        "caret at column {x} rendered outside the dialog (right edge {dialog_right})"
                    );
                }
            }
        }
        assert!(found_reversed, "expected a reversed caret cell somewhere in the buffer");
    }

    /// On a normal-sized 80x24 terminal, submitting an empty form must show
    /// the validation error alongside the hint row — not silently drop it
    /// past the scroll window.
    #[test]
    fn validation_error_is_visible_on_a_standard_terminal() {
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        let mut form = ProfileForm::new(None);

        assert!(form.submit().is_none());
        assert_eq!(form.error_msg.as_deref(), Some("Name is required"));

        form.render(area, &mut buf);

        let text = buffer_text(&buf);
        assert!(
            text.contains("Name is required"),
            "validation error should be visible, got:\n{text}"
        );
        assert!(
            text.contains("Esc") && text.contains("cancel"),
            "hint row should still be pinned and visible, got:\n{text}"
        );
    }
}
