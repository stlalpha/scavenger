use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

use crate::tui::colors;

/// Vertical splitter state — drag left/right to resize sibling panes.
///
/// The splitter tracks whether a drag is active and the initial mouse/size
/// state at capture time. The parent layout reads `left_width` after each
/// mouse event to recompute the constraint split.
#[derive(Debug)]
pub struct VSplitterState {
    dragging: bool,
    hovering: bool,
    start_x: u16,
    initial_left: u16,
    /// Current width of the left pane. The parent owns the Rect split but
    /// reads this value to set constraints.
    pub left_width: u16,
    pub min_left: u16,
    pub min_right: u16,
}

impl VSplitterState {
    pub fn new(initial_left: u16, min_left: u16, min_right: u16) -> Self {
        Self {
            dragging: false,
            hovering: false,
            start_x: 0,
            initial_left,
            left_width: initial_left,
            min_left,
            min_right,
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn on_mouse_down(&mut self, screen_x: u16, current_left_width: u16) {
        self.dragging = true;
        self.start_x = screen_x;
        self.initial_left = current_left_width;
    }

    /// Returns true if the left_width was updated.
    pub fn on_mouse_move(&mut self, screen_x: u16, parent_width: u16) -> bool {
        if !self.dragging {
            return false;
        }
        let dx = screen_x as i32 - self.start_x as i32;
        let new_left = (self.initial_left as i32 + dx).max(0) as u16;
        if new_left < self.min_left {
            return false;
        }
        // 1 column for the splitter bar itself
        let remaining = parent_width.saturating_sub(new_left + 1);
        if remaining < self.min_right {
            return false;
        }
        self.left_width = new_left;
        true
    }

    pub fn on_mouse_up(&mut self) {
        self.dragging = false;
    }

    pub fn set_hover(&mut self, hover: bool) {
        self.hovering = hover;
    }

    fn color(&self) -> Color {
        if self.dragging {
            colors::ORANGE
        } else if self.hovering {
            colors::SPLITTER_HOVER
        } else {
            colors::SPLITTER_IDLE
        }
    }
}

pub struct VSplitterWidget<'a> {
    state: &'a VSplitterState,
}

impl<'a> VSplitterWidget<'a> {
    pub fn new(state: &'a VSplitterState) -> Self {
        Self { state }
    }
}

impl Widget for VSplitterWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default().bg(self.state.color());
        for y in area.top()..area.bottom() {
            if let Some(cell) = buf.cell_mut((area.x, y)) {
                cell.set_style(style);
                cell.set_char(' ');
            }
        }
    }
}

/// Horizontal splitter state — drag up/down to resize sibling panes.
#[derive(Debug)]
pub struct HSplitterState {
    dragging: bool,
    hovering: bool,
    start_y: u16,
    initial_bottom: u16,
    /// Current height of the bottom pane.
    pub bottom_height: u16,
    pub min_top: u16,
    pub min_bottom: u16,
}

impl HSplitterState {
    pub fn new(initial_bottom: u16, min_top: u16, min_bottom: u16) -> Self {
        Self {
            dragging: false,
            hovering: false,
            start_y: 0,
            initial_bottom,
            bottom_height: initial_bottom,
            min_top,
            min_bottom,
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn on_mouse_down(&mut self, screen_y: u16, current_bottom_height: u16) {
        self.dragging = true;
        self.start_y = screen_y;
        self.initial_bottom = current_bottom_height;
    }

    /// Returns true if the bottom_height was updated.
    pub fn on_mouse_move(&mut self, screen_y: u16, parent_height: u16) -> bool {
        if !self.dragging {
            return false;
        }
        let dy = screen_y as i32 - self.start_y as i32;
        let new_bottom = (self.initial_bottom as i32 - dy).max(0) as u16;
        if new_bottom < self.min_bottom {
            return false;
        }
        let remaining = parent_height.saturating_sub(new_bottom + 1);
        if remaining < self.min_top {
            return false;
        }
        self.bottom_height = new_bottom;
        true
    }

    pub fn on_mouse_up(&mut self) {
        self.dragging = false;
    }

    pub fn set_hover(&mut self, hover: bool) {
        self.hovering = hover;
    }

    fn color(&self) -> Color {
        if self.dragging {
            colors::ORANGE
        } else if self.hovering {
            colors::SPLITTER_HOVER
        } else {
            colors::SPLITTER_IDLE
        }
    }
}

pub struct HSplitterWidget<'a> {
    state: &'a HSplitterState,
}

impl<'a> HSplitterWidget<'a> {
    pub fn new(state: &'a HSplitterState) -> Self {
        Self { state }
    }
}

impl Widget for HSplitterWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default().bg(self.state.color());
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, area.y)) {
                cell.set_style(style);
                cell.set_char(' ');
            }
        }
    }
}
