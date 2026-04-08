pub mod data;
pub mod messages;
pub mod screens;
pub mod widgets;

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::config::AppConfig;
use crate::db::Database;
use crate::models::{Listing, Profile};
use crate::tui::data::DataLayer;
use crate::tui::messages::AppAction;
use crate::tui::screens::main::{render_main_screen, MainLayout};
use crate::tui::widgets::log_panel::LogPanelState;
use crate::tui::widgets::splitter::{HSplitterState, VSplitterState};
use crate::tui::widgets::status_bar::StatusBarState;

const DB_POLL_INTERVAL: Duration = Duration::from_secs(2);
const EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(100);
const LOG_POLL_INTERVAL: Duration = Duration::from_millis(800);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Profiles,
    Feed,
    Detail,
    Log,
}

impl FocusedPanel {
    fn next(self) -> Self {
        match self {
            Self::Profiles => Self::Feed,
            Self::Feed => Self::Detail,
            Self::Detail => Self::Log,
            Self::Log => Self::Profiles,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Profiles => Self::Log,
            Self::Feed => Self::Profiles,
            Self::Detail => Self::Feed,
            Self::Log => Self::Detail,
        }
    }
}

pub struct App {
    config: AppConfig,
    db: Database,
    profiles: Vec<Profile>,
    active_profile_id: Option<String>,
    listings: Vec<Listing>,
    selected_index: Option<usize>,
    profile_stats: HashMap<String, u32>,
    focused: FocusedPanel,
    running: bool,
    shutdown_daemon: bool,

    // Splitter state
    vsplit1: VSplitterState,
    vsplit2: VSplitterState,
    hsplit: HSplitterState,

    // Widget state
    log_state: LogPanelState,
    status_state: StatusBarState,

    // Timers
    last_db_poll: Instant,
    last_log_poll: Instant,
}

impl App {
    pub fn new(config: AppConfig) -> Result<Self, String> {
        let db_path = config.db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create db dir: {e}"))?;
        }
        let db = Database::open(&db_path)
            .map_err(|e| format!("open db: {e}"))?;
        db.migrate().map_err(|e| format!("migrate db: {e}"))?;

        let active_profile_id = config.profiles.first().map(|p| p.id.clone());
        let profiles = config.profiles.clone();
        let log_path = config.log_path();

        Ok(Self {
            config,
            db,
            profiles,
            active_profile_id,
            listings: Vec::new(),
            selected_index: None,
            profile_stats: HashMap::new(),
            focused: FocusedPanel::Feed,
            running: true,
            shutdown_daemon: false,
            vsplit1: VSplitterState::new(24, 14, 20),
            vsplit2: VSplitterState::new(0, 20, 20), // computed dynamically
            hsplit: HSplitterState::new(10, 5, 3),
            log_state: LogPanelState::new(log_path),
            status_state: StatusBarState::default(),
            last_db_poll: Instant::now() - DB_POLL_INTERVAL, // force immediate poll
            last_log_poll: Instant::now(),
        })
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.main_loop(&mut terminal);

        // Cleanup — always restore terminal state
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        if self.shutdown_daemon {
            self.send_daemon_command(r#"{"command":"shutdown"}"#);
        }

        result
    }

    fn main_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        while self.running {
            // Render
            terminal.draw(|frame| {
                let area = frame.area();
                render_main_screen(
                    area,
                    frame.buffer_mut(),
                    &self.profiles,
                    self.active_profile_id.as_deref(),
                    &self.profile_stats,
                    &self.listings,
                    self.selected_index,
                    self.focused,
                    &self.vsplit1,
                    &self.vsplit2,
                    &self.hsplit,
                    &self.log_state,
                    &self.status_state,
                );
            })?;

            // Poll events
            if event::poll(EVENT_POLL_TIMEOUT)? {
                match event::read()? {
                    Event::Key(key) => {
                        if let Some(action) = self.map_key(key) {
                            self.handle_action(action);
                        }
                    }
                    Event::Mouse(mouse) => {
                        let s = terminal.size()?;
                        let rect = ratatui::layout::Rect::new(0, 0, s.width, s.height);
                        self.handle_mouse(mouse, rect);
                    }
                    Event::Resize(_, _) => {
                        // Ratatui redraws on next frame automatically.
                    }
                    _ => {}
                }
            }

            // Periodic DB poll
            if self.last_db_poll.elapsed() >= DB_POLL_INTERVAL {
                self.poll_data();
                self.last_db_poll = Instant::now();
            }

            // Periodic log tail
            if self.last_log_poll.elapsed() >= LOG_POLL_INTERVAL {
                self.log_state.poll();
                self.last_log_poll = Instant::now();
            }

            // Tick spinner
            self.status_state.tick();
        }

        Ok(())
    }

    fn map_key(&self, key: KeyEvent) -> Option<AppAction> {
        match key.code {
            KeyCode::Char('q') if key.modifiers.is_empty() => Some(AppAction::Quit),
            KeyCode::Char('Q') => Some(AppAction::QuitAll),
            KeyCode::Tab => Some(AppAction::FocusNext),
            KeyCode::BackTab => Some(AppAction::FocusPrev),
            KeyCode::Right if key.modifiers.is_empty() => Some(AppAction::FocusNext),
            KeyCode::Left if key.modifiers.is_empty() => Some(AppAction::FocusPrev),
            KeyCode::Char('o') => Some(AppAction::OpenUrl),
            KeyCode::Char('s') => Some(AppAction::SaveListing),
            KeyCode::Char('d') => Some(AppAction::DismissListing),
            KeyCode::Char('n') => Some(AppAction::SnoozeListing),
            KeyCode::Char('j') | KeyCode::Down => Some(AppAction::NavigateDown),
            KeyCode::Char('k') | KeyCode::Up => Some(AppAction::NavigateUp),
            KeyCode::Enter => {
                // Select current listing (mark seen)
                if let Some(idx) = self.selected_index {
                    if let Some(l) = self.listings.get(idx) {
                        return Some(AppAction::SelectListing(l.id.clone()));
                    }
                }
                None
            }
            KeyCode::Char('a') => Some(AppAction::AddProfile),
            KeyCode::Char('e') => Some(AppAction::EditProfile),
            KeyCode::Char('r') => Some(AppAction::Repoll),
            KeyCode::Char('?') => Some(AppAction::ShowHelp),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(AppAction::Quit)
            }
            _ => None,
        }
    }

    fn handle_action(&mut self, action: AppAction) {
        match action {
            AppAction::Quit => {
                self.running = false;
            }
            AppAction::QuitAll => {
                self.shutdown_daemon = true;
                self.running = false;
            }
            AppAction::FocusNext => {
                self.focused = self.focused.next();
            }
            AppAction::FocusPrev => {
                self.focused = self.focused.prev();
            }
            AppAction::NavigateDown => {
                if self.listings.is_empty() {
                    return;
                }
                self.selected_index = Some(match self.selected_index {
                    Some(i) if i + 1 < self.listings.len() => i + 1,
                    Some(i) => i,
                    None => 0,
                });
            }
            AppAction::NavigateUp => {
                if self.listings.is_empty() {
                    return;
                }
                self.selected_index = Some(match self.selected_index {
                    Some(0) | None => 0,
                    Some(i) => i - 1,
                });
            }
            AppAction::OpenUrl => {
                if let Some(listing) = self.selected_listing() {
                    let url = listing.url.clone();
                    let opener = if cfg!(target_os = "macos") {
                        "open"
                    } else {
                        "xdg-open"
                    };
                    let _ = std::process::Command::new(opener).arg(&url).spawn();
                }
            }
            AppAction::SaveListing => {
                self.mark_selected("saved");
            }
            AppAction::DismissListing => {
                self.mark_selected("dismissed");
            }
            AppAction::SnoozeListing => {
                self.mark_selected("snoozed");
            }
            AppAction::SelectListing(ref id) => {
                let dl = DataLayer::new(&self.db);
                let _ = dl.mark_seen(id);
            }
            AppAction::SelectProfile(ref id) => {
                self.active_profile_id = Some(id.clone());
                self.selected_index = None;
                self.poll_data();
            }
            AppAction::Repoll => {
                if let Some(ref pid) = self.active_profile_id {
                    let cmd = format!(r#"{{"command":"poll","profile_id":"{}"}}"#, pid);
                    self.send_daemon_command(&cmd);
                }
            }
            AppAction::AddProfile | AppAction::EditProfile => {
                // Profile management screens are a future addition.
            }
            AppAction::ShowHelp => {
                // Help is shown inline in the status bar / detail panel.
            }
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, _size: ratatui::layout::Rect) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if click is on a splitter bar. We use the last-rendered
                // layout to determine hit testing. For simplicity, we store the
                // splitter column/row positions and test against them.
                // The actual areas are recomputed each frame; we approximate here.
                let layout = MainLayout::compute(
                    _size,
                    self.vsplit1.left_width,
                    self.hsplit.bottom_height,
                );
                if mouse.column == layout.vsplit1.x {
                    self.vsplit1.on_mouse_down(mouse.column, layout.sidebar.width);
                } else if mouse.column == layout.vsplit2.x {
                    self.vsplit2
                        .on_mouse_down(mouse.column, layout.sidebar.width + 1 + layout.feed.width);
                } else if mouse.row == layout.hsplit.y {
                    self.hsplit.on_mouse_down(mouse.row, layout.log.height);
                } else {
                    // Click in a panel — update focus
                    if layout.sidebar.contains((mouse.column, mouse.row).into()) {
                        self.focused = FocusedPanel::Profiles;
                        // Select profile by row
                        let row = (mouse.row - layout.sidebar.y) as usize;
                        if row < self.profiles.len() {
                            let pid = self.profiles[row].id.clone();
                            self.handle_action(AppAction::SelectProfile(pid));
                        }
                    } else if layout.feed.contains((mouse.column, mouse.row).into()) {
                        self.focused = FocusedPanel::Feed;
                        let row = (mouse.row - layout.feed.y) as usize;
                        if row < self.listings.len() {
                            self.selected_index = Some(row);
                        }
                    } else if layout.detail.contains((mouse.column, mouse.row).into()) {
                        self.focused = FocusedPanel::Detail;
                    } else if layout.log.contains((mouse.column, mouse.row).into()) {
                        self.focused = FocusedPanel::Log;
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Moved => {
                let size = _size;
                if self.vsplit1.is_dragging() {
                    self.vsplit1.on_mouse_move(mouse.column, size.width);
                } else if self.vsplit2.is_dragging() {
                    self.vsplit2.on_mouse_move(mouse.column, size.width);
                } else if self.hsplit.is_dragging() {
                    self.hsplit.on_mouse_move(mouse.row, size.height);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.vsplit1.on_mouse_up();
                self.vsplit2.on_mouse_up();
                self.hsplit.on_mouse_up();
            }
            _ => {}
        }
    }

    fn selected_listing(&self) -> Option<&Listing> {
        self.selected_index.and_then(|i| self.listings.get(i))
    }

    fn mark_selected(&mut self, status: &str) {
        let id = match self.selected_listing() {
            Some(l) => l.id.clone(),
            None => return,
        };
        let dl = DataLayer::new(&self.db);
        let _ = dl.mark_status(&id, status);
        self.poll_data();
    }

    fn poll_data(&mut self) {
        let dl = DataLayer::new(&self.db);
        if let Ok(listings) = dl.get_listings(self.active_profile_id.as_deref(), 100) {
            self.listings = listings;
            // Clamp selected index
            if let Some(idx) = self.selected_index {
                if idx >= self.listings.len() {
                    self.selected_index = if self.listings.is_empty() {
                        None
                    } else {
                        Some(self.listings.len() - 1)
                    };
                }
            }
        }
        if let Ok(stats) = dl.get_profile_stats() {
            let total: u32 = stats.values().sum();
            self.status_state.new_count = total;
            self.profile_stats = stats;
        }
        if let Ok(states) = dl.get_source_states() {
            self.status_state.source_states = states;
        }
        if let Ok(Some(ts)) = dl.get_last_source_poll() {
            self.status_state.last_poll_iso = Some(ts);
        }

        // Check daemon status
        let daemon_status = self.check_daemon_status();
        self.status_state.daemon_up = daemon_status.0;
        self.status_state.active_polls = daemon_status.1;

        // Update poll interval from active profile
        if let Some(ref pid) = self.active_profile_id {
            if let Some(p) = self.profiles.iter().find(|p| &p.id == pid) {
                self.status_state.poll_interval_sec = p.poll_interval_sec;
            }
        }
    }

    fn check_daemon_status(&self) -> (bool, Vec<String>) {
        let socket_path = self.config.socket_path();
        let Ok(mut stream) = UnixStream::connect(&socket_path) else {
            return (false, vec![]);
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .ok();
        let cmd = b"{\"command\":\"status\"}\n";
        if stream.write_all(cmd).is_err() {
            return (false, vec![]);
        }
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.contains(&b'\n') {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let Ok(resp) = serde_json::from_slice::<serde_json::Value>(&buf) else {
            return (false, vec![]);
        };
        if resp.get("status").and_then(|v| v.as_str()) != Some("ok") {
            return (false, vec![]);
        }
        let data = resp.get("data").cloned().unwrap_or(serde_json::Value::Null);
        let active: Vec<String> = data
            .get("active_polls")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        (true, active)
    }

    fn send_daemon_command(&self, json_line: &str) {
        let socket_path = self.config.socket_path();
        if let Ok(mut stream) = UnixStream::connect(&socket_path) {
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .ok();
            let mut msg = json_line.as_bytes().to_vec();
            if !msg.ends_with(b"\n") {
                msg.push(b'\n');
            }
            let _ = stream.write_all(&msg);
        }
    }
}
