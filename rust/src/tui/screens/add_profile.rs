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
        if self.editing.is_none() {
            return None;
        }
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

        // Lay out fields vertically
        let mut y = inner.y;
        let field_area = |y: &mut u16, height: u16| -> Rect {
            let r = Rect::new(inner.x + 1, *y, inner.width.saturating_sub(2), height);
            *y += height;
            r
        };

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
            let inner = block.inner(area);
            Widget::render(block, area, buf);
            Paragraph::new(value).style(style).render(inner, buf);
            // Show cursor
            if focused && inner.width > 0 {
                let cx = inner.x + (cursor_pos as u16).min(inner.width - 1);
                if let Some(cell) = buf.cell_mut((cx, inner.y)) {
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

        // Name
        if y < inner.y + inner.height {
            render_label(field_area(&mut y, 1), buf, "Name");
            render_input(
                field_area(&mut y, 2),
                buf,
                &self.name,
                self.focused == FormField::Name,
                self.cursor_pos,
            );
        }

        // Keywords
        if y < inner.y + inner.height {
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

        // Negative keywords
        if y < inner.y + inner.height {
            render_label(field_area(&mut y, 1), buf, "Negative keywords");
            render_input(
                field_area(&mut y, 2),
                buf,
                &self.negative_keywords,
                self.focused == FormField::NegativeKeywords,
                self.cursor_pos,
            );
        }

        // Sources
        if y < inner.y + inner.height {
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
            y += 1; // spacer
        }

        // Price range
        if y < inner.y + inner.height {
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

        // Poll interval + Priority
        if y < inner.y + inner.height {
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
            y += 1; // spacer
        }

        // Tags
        if y < inner.y + inner.height {
            render_label(field_area(&mut y, 1), buf, "Tags");
            render_input(
                field_area(&mut y, 2),
                buf,
                &self.tags,
                self.focused == FormField::Tags,
                self.cursor_pos,
            );
        }

        // Escalation keywords
        if y < inner.y + inner.height {
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

        // Location radius
        if y < inner.y + inner.height {
            render_label(field_area(&mut y, 1), buf, "Location radius (miles)");
            render_input(
                field_area(&mut y, 2),
                buf,
                &self.location_radius,
                self.focused == FormField::LocationRadius,
                self.cursor_pos,
            );
        }

        // Error message
        if let Some(ref err) = self.error_msg {
            if y < inner.y + inner.height {
                y += 1;
                let err_area = field_area(&mut y, 1);
                Paragraph::new(Span::styled(
                    err.clone(),
                    Style::default().fg(colors::PINK),
                ))
                .render(err_area, buf);
            }
        }

        // Delete confirmation overlay
        if self.confirm_delete {
            if y + 2 < inner.y + inner.height {
                y += 1;
                let confirm_area = field_area(&mut y, 2);
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        "Delete this profile and all its listings?",
                        Style::default().fg(colors::PINK),
                    )),
                    Line::from(vec![
                        Span::styled("y", Style::default().fg(colors::ORANGE)),
                        Span::styled("es", Style::default().fg(colors::TEXT_DIM)),
                        Span::raw("  "),
                        Span::styled("n", Style::default().fg(colors::ORANGE)),
                        Span::styled("o", Style::default().fg(colors::TEXT_DIM)),
                    ]),
                ])
                .render(confirm_area, buf);
            }
        }

        // Bottom buttons hint
        let bottom_y = (inner.y + inner.height).saturating_sub(1);
        if bottom_y > y {
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
}
