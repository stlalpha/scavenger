pub mod colors;
pub mod data;
pub mod messages;
pub mod screens;
pub mod widgets;

use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc;
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
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use ratatui::Terminal;

use crate::config::{self, AppConfig};
use crate::db::Database;
use crate::models::{AlertPriority, Listing, Profile};
use crate::plugins::images::fetch_listing_images;
use crate::tui::data::DataLayer;
use crate::tui::messages::AppAction;
use crate::tui::screens::add_profile::{render_confirm_overlay, FormResult, ProfileForm, ProfileFormData};
use crate::tui::screens::main::{render_main_screen, HeroImageCache, MainLayout};
use crate::tui::widgets::detail_panel::DetailPanel;
use crate::tui::widgets::log_panel::LogPanelState;
use crate::tui::widgets::profile_sidebar::ProfileSidebar;
use crate::tui::widgets::results_feed::ResultsFeed;
use crate::tui::widgets::splitter::{HSplitterState, VSplitterState};
use crate::tui::widgets::status_bar::StatusBarState;
use crate::tui::widgets::thumbnail::ThumbnailCache;

const DB_POLL_INTERVAL: Duration = Duration::from_secs(2);
const EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(100);
const LOG_POLL_INTERVAL: Duration = Duration::from_millis(800);
/// How long to wait after a selection settles before fetching the full
/// image gallery for the detail page (mirrors the Python TUI's debounce).
const GALLERY_DEBOUNCE: Duration = Duration::from_millis(1500);

/// (daemon_up, active poll source ids, profile ids the daemon currently
/// knows about, (source_id, blocked url) pairs from bot_blocks) — see
/// `App::check_daemon_status`.
type DaemonStatusReport = (
    bool,
    Vec<String>,
    Vec<String>,
    Vec<(String, String)>,
    Option<AiStatus>,
    // (profile_id, source_id) pairs currently being polled — one at a time
    // under the sequential scheduler.
    Vec<(String, String)>,
);

/// AI evaluation health from `data.ai` in the daemon status response.
/// `None` when the daemon is down or predates the field.
#[derive(Clone, Debug, PartialEq)]
pub struct AiStatus {
    pub enabled: bool,
    pub healthy: bool,
    pub detail: String,
}

/// A gallery fetch job — listing id, detail-page URL, and source plugin id.
struct GalleryJob {
    listing_id: String,
    url: String,
    source_id: String,
}

/// A completed job, sent back from the worker threads.
enum ImgResult {
    Thumbnail { url: String, path: Option<PathBuf> },
    Gallery { listing_id: String, images: Vec<String> },
}

/// Selection whose gallery fetch is debounced — armed on selection change,
/// fired once `is_ready` returns true on a later tick.
struct PendingGallery {
    listing_id: String,
    url: String,
    source_id: String,
    requested_at: Instant,
}

impl PendingGallery {
    fn is_ready(&self, debounce: Duration) -> bool {
        self.requested_at.elapsed() >= debounce
    }
}

/// Thumbnail worker — owns the on-disk/memory image cache, so concurrent
/// selections can never race on the same download. Runs on its own thread,
/// separate from the gallery worker, so a slow 15s gallery scrape can never
/// stall every hero thumbnail behind it in a single FIFO queue.
fn run_thumb_worker(rx: mpsc::Receiver<String>, tx: mpsc::Sender<ImgResult>, cache_dir: PathBuf) {
    let mut cache = ThumbnailCache::new(cache_dir);
    while let Ok(url) = rx.recv() {
        let path = cache.download_sync(&url);
        let _ = tx.send(ImgResult::Thumbnail { url, path });
    }
}

/// Gallery worker — fetches a listing's full detail-page image set. Kept on
/// its own thread/queue so it never blocks thumbnail downloads.
fn run_gallery_worker(rx: mpsc::Receiver<GalleryJob>, tx: mpsc::Sender<ImgResult>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();

    while let Ok(job) = rx.recv() {
        let images = match &rt {
            Ok(rt) => rt
                .block_on(fetch_listing_images(&job.url, &job.source_id))
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        if !images.is_empty() {
            let _ = tx.send(ImgResult::Gallery {
                listing_id: job.listing_id,
                images,
            });
        }
    }
}

/// Mouse hit-testing against a splitter bar's column/row, with ±1 tolerance
/// — an exact match is unreliable on some terminals' mouse reporting.
fn near(actual: u16, target: u16) -> bool {
    actual.abs_diff(target) <= 1
}

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

/// Which screen owns key events and rendering. `ProfileForm` holds its own
/// delete-confirmation sub-state (`ProfileForm::confirm_delete`), so it does
/// double duty as both the add/edit and delete-confirm screens — both cases
/// need the same modal to own all key input until Esc or a submit.
/// `ConfirmQuitAll` is a lightweight standalone overlay guarding the
/// daemon-shutdown quit ('Q').
enum Screen {
    Main,
    ProfileForm(Box<ProfileForm>),
    ConfirmQuitAll,
    Help,
}

pub struct App {
    config: AppConfig,
    /// Resolved path of the config file this session was started with
    /// (`--config`, or the default) — profile CRUD always writes here so a
    /// custom-config session never silently edits the default file.
    config_path: PathBuf,
    db: Database,
    focused: FocusedPanel,
    screen: Screen,
    running: bool,
    shutdown_daemon: bool,

    // Panel widgets — each owns its own selection/scroll/sort state.
    sidebar: ProfileSidebar,
    feed: ResultsFeed,
    detail: DetailPanel,

    // Splitter state
    vsplit1: VSplitterState,
    vsplit2: VSplitterState,
    hsplit: HSplitterState,

    // Widget state
    thumb_cache: ThumbnailCache,
    hero_cache: HeroImageCache,
    log_state: LogPanelState,
    status_state: StatusBarState,

    // Image download workers — thumbnails and galleries run on separate
    // threads/queues (see `run_thumb_worker`/`run_gallery_worker`) so a slow
    // gallery scrape never stalls hero thumbnails, but both report back
    // through the same result channel.
    img_tx: mpsc::Sender<String>,
    gallery_tx: mpsc::Sender<GalleryJob>,
    img_rx: mpsc::Receiver<ImgResult>,
    /// URL of the download the detail panel is currently waiting on —
    /// distinct from `pending_thumb_urls`, which dedups in-flight jobs.
    pending_img_url: Option<String>,
    pending_thumb_urls: HashSet<String>,
    gallery_pending: Option<PendingGallery>,
    /// Per-listing gallery results, so revisiting a listing reuses the
    /// already-scraped image set instead of re-fetching it.
    gallery_cache: HashMap<String, Vec<String>>,

    // Timers
    last_db_poll: Instant,
    last_log_poll: Instant,
}

impl App {
    pub fn new(config: AppConfig, config_path: PathBuf) -> Result<Self, String> {
        let db_path = config.db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create db dir: {e}"))?;
        }
        let db = Database::open(&db_path.to_string_lossy())
            .map_err(|e| format!("open db: {e}"))?;
        db.init().map_err(|e| format!("init db: {e}"))?;
        db.migrate().map_err(|e| format!("migrate db: {e}"))?;

        let sidebar = ProfileSidebar::new(config.profiles.clone());
        let log_path = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("scavenger")
            .join("daemon.log");
        let (img_tx, thumb_rx) = mpsc::channel::<String>();
        let (gallery_tx, gallery_rx) = mpsc::channel::<GalleryJob>();
        let (worker_tx, img_rx) = mpsc::channel::<ImgResult>();
        let cache_dir = ThumbnailCache::default_cache_dir();
        let thumb_result_tx = worker_tx.clone();
        std::thread::spawn(move || run_thumb_worker(thumb_rx, thumb_result_tx, cache_dir));
        std::thread::spawn(move || run_gallery_worker(gallery_rx, worker_tx));

        Ok(Self {
            config,
            config_path,
            db,
            focused: FocusedPanel::Feed,
            screen: Screen::Main,
            running: true,
            shutdown_daemon: false,
            sidebar,
            feed: ResultsFeed::new(),
            detail: DetailPanel::new(),
            vsplit1: VSplitterState::new(24, 14, 20),
            vsplit2: VSplitterState::new(0, 20, 20), // computed dynamically
            hsplit: HSplitterState::new(10, 5, 3),
            thumb_cache: ThumbnailCache::with_default_dir(),
            hero_cache: HeroImageCache::new(),
            img_tx,
            gallery_tx,
            img_rx,
            pending_img_url: None,
            pending_thumb_urls: HashSet::new(),
            gallery_pending: None,
            gallery_cache: HashMap::new(),
            log_state: LogPanelState::new(log_path),
            status_state: StatusBarState::default(),
            last_db_poll: Instant::now() - DB_POLL_INTERVAL, // force immediate poll
            last_log_poll: Instant::now(),
        })
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        // Negotiate the terminal's graphics protocol (Kitty/iTerm2/Sixel)
        // while raw mode is on but before the alternate screen, so hero
        // images render at native resolution wherever the terminal allows.
        self.hero_cache.detect_terminal();
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
            // Check for completed background image jobs.
            while let Ok(result) = self.img_rx.try_recv() {
                match result {
                    ImgResult::Thumbnail { url, path } => {
                        // Failure still removes the marker below, so
                        // re-selecting the same URL retries the download.
                        self.pending_thumb_urls.remove(&url);
                        if let Some(p) = &path {
                            self.thumb_cache.insert_mem(url.clone(), p.clone());
                        }
                        if self.pending_img_url.as_deref() == Some(url.as_str()) {
                            self.pending_img_url = None;
                            // Only swap in a download that actually succeeded —
                            // a failed fetch (path None) must never blank an
                            // already-displayed hero image.
                            if path.is_some() {
                                self.detail.hero_image_path = path;
                            }
                        }
                    }
                    ImgResult::Gallery { listing_id, images } => {
                        self.gallery_cache.insert(listing_id.clone(), images.clone());
                        // Discard if the selection moved on while this was in flight.
                        if self.detail.current_listing().map(|l| l.id.as_str())
                            == Some(listing_id.as_str())
                        {
                            self.detail.set_gallery(images);
                            self.refresh_hero_image(false);
                        }
                    }
                }
            }

            // Update the detail panel when the focused listing changes.
            let selected = self.feed.focused_listing().cloned();
            if self.detail.show_listing(selected) {
                self.refresh_hero_image(true);
                self.gallery_pending = self.detail.current_listing().map(|l| PendingGallery {
                    listing_id: l.id.clone(),
                    url: l.url.clone(),
                    source_id: l.source_id.clone(),
                    requested_at: Instant::now(),
                });
            }

            // Fire the debounced full-gallery fetch once the selection settles.
            if let Some(pending) = self.gallery_pending.take() {
                if pending.is_ready(GALLERY_DEBOUNCE) {
                    self.fire_or_serve_gallery(pending);
                } else {
                    self.gallery_pending = Some(pending);
                }
            }

            let img_path = self.detail.hero_image_path.clone();
            terminal.draw(|frame| {
                let area = frame.area();
                render_main_screen(
                    area,
                    frame.buffer_mut(),
                    &mut self.sidebar,
                    &mut self.feed,
                    &self.detail,
                    self.focused,
                    &self.vsplit1,
                    &self.vsplit2,
                    &self.hsplit,
                    &self.log_state,
                    &self.status_state,
                    img_path.as_deref(),
                    &mut self.hero_cache,
                );
                match &self.screen {
                    Screen::ProfileForm(form) => form.render(area, frame.buffer_mut()),
                    Screen::ConfirmQuitAll => render_confirm_overlay(
                        area,
                        frame.buffer_mut(),
                        &[
                            Line::from(Span::styled(
                                "Shut down the daemon and quit?",
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
                    ),
                    Screen::Help => render_help_overlay(area, frame.buffer_mut()),
                    Screen::Main => {}
                }
            })?;

            // Poll events
            if event::poll(EVENT_POLL_TIMEOUT)? {
                match event::read()? {
                    Event::Key(key) => match &self.screen {
                        Screen::ProfileForm(_) => self.handle_form_key(key),
                        Screen::ConfirmQuitAll => self.handle_quit_confirm_key(key),
                        Screen::Help => self.screen = Screen::Main,
                        Screen::Main => {
                            if key.code == KeyCode::Char('Q') {
                                self.screen = Screen::ConfirmQuitAll;
                            } else if key.code == KeyCode::Char('x')
                                && !self.status_state.bot_blocks.is_empty()
                            {
                                self.open_blocked_urls();
                            } else if let Some(action) = self.map_key(key) {
                                self.handle_action(action);
                            }
                        }
                    },
                    Event::Mouse(mouse) => {
                        // Modals own all input while open — a click/drag
                        // must never fall through to the screen beneath.
                        if matches!(self.screen, Screen::Main) {
                            let s = terminal.size()?;
                            let rect = ratatui::layout::Rect::new(0, 0, s.width, s.height);
                            self.handle_mouse(mouse, rect);
                        }
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

    /// 'Q' is intercepted before this is reached (see `main_loop`) to route
    /// through the quit-confirmation overlay instead of firing directly.
    fn map_key(&self, key: KeyEvent) -> Option<AppAction> {
        match key.code {
            KeyCode::Char('q') if key.modifiers.is_empty() => Some(AppAction::Quit),
            KeyCode::Tab => Some(AppAction::FocusNext),
            KeyCode::BackTab => Some(AppAction::FocusPrev),
            KeyCode::Right if key.modifiers.is_empty() => Some(AppAction::FocusNext),
            KeyCode::Left if key.modifiers.is_empty() => Some(AppAction::FocusPrev),
            // Listing actions only make sense scoped to Feed/Detail — from
            // the Profiles or Log panel these keys are simply unmapped
            // rather than acting on whatever the feed happens to have
            // selected.
            KeyCode::Char('o') if self.listing_actions_active() => Some(AppAction::OpenUrl),
            KeyCode::Char('s') if self.listing_actions_active() => Some(AppAction::SaveListing),
            KeyCode::Char('d') if self.listing_actions_active() => Some(AppAction::DismissListing),
            KeyCode::Char('n') if self.listing_actions_active() => Some(AppAction::SnoozeListing),
            KeyCode::Char('j') | KeyCode::Down => Some(AppAction::NavigateDown),
            KeyCode::Char('k') | KeyCode::Up => Some(AppAction::NavigateUp),
            KeyCode::Enter => {
                // Select current listing (mark seen)
                self.selected_listing()
                    .map(|l| AppAction::SelectListing(l.id.clone()))
            }
            KeyCode::Char('a') => Some(AppAction::AddProfile),
            KeyCode::Char('e') => Some(AppAction::EditProfile),
            KeyCode::Char('r') => Some(AppAction::Repoll),
            KeyCode::Char('S') => Some(AppAction::CycleSort),
            KeyCode::Char('.') => Some(AppAction::NextImage),
            KeyCode::Char(',') => Some(AppAction::PrevImage),
            KeyCode::Char('?') => Some(AppAction::ShowHelp),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(AppAction::Quit)
            }
            _ => None,
        }
    }

    /// Whether o/s/d/n should act on the feed's current selection — only
    /// when the Feed or Detail panel actually has focus. Guards against
    /// e.g. 'd' from the Profiles panel dismissing an arbitrary listing.
    fn listing_actions_active(&self) -> bool {
        matches!(self.focused, FocusedPanel::Feed | FocusedPanel::Detail)
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
            AppAction::NavigateDown => match self.focused {
                FocusedPanel::Profiles => {
                    self.sidebar.select_next();
                    self.refresh_after_profile_change();
                }
                _ => self.feed.select_next(),
            },
            AppAction::NavigateUp => match self.focused {
                FocusedPanel::Profiles => {
                    self.sidebar.select_prev();
                    self.refresh_after_profile_change();
                }
                _ => self.feed.select_prev(),
            },
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
                if let Some(idx) = self.sidebar.profiles().iter().position(|p| &p.id == id) {
                    self.sidebar.state.select(Some(idx));
                }
                self.refresh_after_profile_change();
            }
            AppAction::Repoll => {
                if let Some(pid) = self.sidebar.selected_profile_id() {
                    let cmd = format!(r#"{{"command":"poll","profile_id":"{}"}}"#, pid);
                    self.send_daemon_command(&cmd);
                }
            }
            AppAction::AddProfile => {
                self.screen = Screen::ProfileForm(Box::new(ProfileForm::new(None)));
            }
            AppAction::EditProfile => {
                if let Some(p) = self.sidebar.selected_profile().cloned() {
                    self.screen = Screen::ProfileForm(Box::new(ProfileForm::new(Some(p))));
                }
            }
            AppAction::ShowHelp => {
                self.screen = Screen::Help;
            }
            AppAction::CycleSort => {
                self.feed.cycle_sort();
            }
            AppAction::NextImage => {
                if self.detail.next_image() {
                    self.refresh_hero_image(true);
                }
            }
            AppAction::PrevImage => {
                if self.detail.prev_image() {
                    self.refresh_hero_image(true);
                }
            }
        }
    }

    /// Common tail of both the keyboard (j/k on Profiles) and mouse
    /// (click-to-select) profile-switch paths — previously only the mouse
    /// path refreshed the feed, so keyboard switching showed the old
    /// profile's listings for up to `DB_POLL_INTERVAL`.
    fn refresh_after_profile_change(&mut self) {
        self.feed.state.select(None);
        self.poll_data();
        self.last_db_poll = Instant::now();
    }

    /// Open every currently bot-blocked source's URL in a visible browser —
    /// the daemon can't solve a bot challenge headlessly, so a human has to.
    fn open_blocked_urls(&self) {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        for (_, url) in &self.status_state.bot_blocks {
            let _ = std::process::Command::new(opener).arg(url).spawn();
        }
    }

    /// Key handling while the profile form modal owns input. Delete-confirm
    /// is a sub-state of the form itself (`confirm_delete`): y/n resolve it,
    /// Esc backs out of it before backing out of the form.
    fn handle_form_key(&mut self, key: KeyEvent) {
        let mut result = None;
        let mut close = false;
        if let Screen::ProfileForm(form) = &mut self.screen {
            match key.code {
                KeyCode::Esc => {
                    if form.confirm_delete {
                        form.cancel_delete();
                    } else {
                        close = true;
                    }
                }
                KeyCode::Char('y') | KeyCode::Char('Y') if form.confirm_delete => {
                    result = form.request_delete();
                }
                KeyCode::Char('n') | KeyCode::Char('N') if form.confirm_delete => {
                    form.cancel_delete();
                }
                KeyCode::Delete if !form.confirm_delete => {
                    form.request_delete();
                }
                KeyCode::Enter if !form.confirm_delete => {
                    result = form.submit();
                }
                KeyCode::Tab if !form.confirm_delete => form.focus_next(),
                KeyCode::BackTab if !form.confirm_delete => form.focus_prev(),
                KeyCode::Left if !form.confirm_delete => form.cursor_left(),
                KeyCode::Right if !form.confirm_delete => form.cursor_right(),
                KeyCode::Backspace if !form.confirm_delete => form.backspace(),
                KeyCode::Char(c) if !form.confirm_delete => form.type_char(c),
                _ => {}
            }
        }

        if close {
            self.screen = Screen::Main;
        } else if let Some(result) = result {
            self.apply_form_result(result);
        }
    }

    /// Key handling while the quit-all confirmation overlay owns input.
    /// Anything but y/Y dismisses back to the main screen.
    fn handle_quit_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => self.handle_action(AppAction::QuitAll),
            _ => self.screen = Screen::Main,
        }
    }

    fn apply_form_result(&mut self, result: FormResult) {
        match result {
            FormResult::Cancel => self.screen = Screen::Main,
            FormResult::Create(data) => self.save_profile(data, false),
            FormResult::Update(data) => self.save_profile(data, true),
            FormResult::Delete { id } => self.delete_profile(&id),
        }
    }

    /// Write the profile via config.rs's toml_edit CRUD, re-read the config
    /// to refresh the in-TUI profile list, then ask the daemon to reload.
    /// CRUD errors and reload-read failures stay on-screen as form error
    /// text rather than closing the modal. An explicit daemon rejection
    /// (bad config, per the daemon's own validation) also keeps the modal
    /// open with the daemon's message; an unreachable daemon is non-fatal
    /// (it may simply be down) since the config write already succeeded.
    fn save_profile(&mut self, data: ProfileFormData, is_update: bool) {
        let alert_priority = match data.alert_priority.as_str() {
            "high" => AlertPriority::High,
            "low" => AlertPriority::Low,
            _ => AlertPriority::Normal,
        };
        // The form has no enabled/disabled control; preserve whatever the
        // profile already had rather than silently re-enabling it.
        let enabled = self
            .sidebar
            .profiles()
            .iter()
            .find(|p| p.id == data.id)
            .map(|p| p.enabled)
            .unwrap_or(true);

        let profile = Profile {
            id: data.id,
            name: data.name,
            keywords: data.keywords,
            negative_keywords: data.negative_keywords,
            sources: data.sources,
            price_min: data.price_min,
            price_max: data.price_max,
            poll_interval_sec: data.poll_interval_sec,
            alert_priority,
            enabled,
            tags: data.tags,
            escalation_keywords: data.escalation_keywords,
            location_radius_mi: data.location_radius_mi,
        };

        let config_path = self.config_path.clone();
        let result = if is_update {
            config::update_profile(&config_path, &profile)
        } else {
            config::append_profile(&config_path, &profile)
        };

        match result {
            Ok(_) => self.finish_crud_write(&config_path),
            Err(e) => {
                if let Screen::ProfileForm(form) = &mut self.screen {
                    form.error_msg = Some(e.to_string());
                }
            }
        }
    }

    /// Deletes the config entry AND the profile's DB rows — the confirm
    /// dialog promises "this profile and all its listings", and leaving
    /// listings behind orphans them: they keep counting toward the status
    /// bar's total, and a future profile reusing the same id would
    /// silently inherit them.
    fn delete_profile(&mut self, id: &str) {
        let config_path = self.config_path.clone();
        match config::delete_profile(&config_path, id) {
            Ok(()) => {
                let _ = self.db.delete_profile_listings(id);
                self.finish_crud_write(&config_path);
            }
            Err(e) => {
                if let Screen::ProfileForm(form) = &mut self.screen {
                    form.confirm_delete = false;
                    form.error_msg = Some(e.to_string());
                }
            }
        }
    }

    /// Common tail of a successful profile CRUD write: re-read the config
    /// (surfacing a read failure instead of dropping it), then ask the
    /// daemon to reload. Only closes the modal once both steps confirm the
    /// new config is actually live — or the daemon is simply unreachable.
    fn finish_crud_write(&mut self, config_path: &std::path::Path) {
        if let Err(e) = self.reload_profiles(config_path) {
            if let Screen::ProfileForm(form) = &mut self.screen {
                form.confirm_delete = false;
                form.error_msg = Some(format!("saved, but reload failed: {e}"));
            }
            return;
        }

        match self.send_daemon_command_checked(r#"{"command":"reload"}"#) {
            Ok(resp) if resp.get("status").and_then(|v| v.as_str()) == Some("ok") => {
                self.screen = Screen::Main;
            }
            Ok(resp) => {
                let msg = resp
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("daemon rejected config")
                    .to_string();
                if let Screen::ProfileForm(form) = &mut self.screen {
                    form.confirm_delete = false;
                    form.error_msg = Some(msg);
                }
            }
            // Daemon unreachable — non-fatal, the config write already
            // succeeded on disk; the daemon will pick it up next reload.
            Err(_) => {
                self.screen = Screen::Main;
            }
        }
    }

    /// Re-read the config file after a CRUD write and refresh the sidebar's
    /// profile list in place (preserving selection where possible).
    fn reload_profiles(&mut self, config_path: &std::path::Path) -> Result<(), String> {
        let new_config = config::load_config(config_path).map_err(|e| e.to_string())?;
        self.sidebar.rebuild(new_config.profiles.clone());
        self.config.profiles = new_config.profiles;
        self.poll_data();
        Ok(())
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
                    self.vsplit2.left_width,
                    self.hsplit.bottom_height,
                );
                if near(mouse.column, layout.vsplit1.x) {
                    self.vsplit1.on_mouse_down(mouse.column, layout.sidebar.width);
                } else if near(mouse.column, layout.vsplit2.x) {
                    self.vsplit2
                        .on_mouse_down(mouse.column, layout.sidebar.width + 1 + layout.feed.width);
                } else if near(mouse.row, layout.hsplit.y) {
                    self.hsplit.on_mouse_down(mouse.row, layout.log.height);
                } else {
                    // Click in a panel — update focus
                    if layout.sidebar.contains((mouse.column, mouse.row).into()) {
                        self.focused = FocusedPanel::Profiles;
                        // Clicking the border/title row just focuses the
                        // panel; only rows below it (checked_sub returns
                        // None on the border) select a profile.
                        if let Some(row) = mouse.row.checked_sub(layout.sidebar.y + 1) {
                            if let Some(idx) = self.sidebar.hit_test(row) {
                                let pid = self.sidebar.profiles()[idx].id.clone();
                                self.handle_action(AppAction::SelectProfile(pid));
                            }
                        }
                    } else if layout.feed.contains((mouse.column, mouse.row).into()) {
                        self.focused = FocusedPanel::Feed;
                        // Clicking the border/title row just focuses the
                        // panel; hit_test (offset + 2-row card height)
                        // only runs on rows below it.
                        if let Some(row) = mouse.row.checked_sub(layout.feed.y + 1) {
                            if let Some(idx) = self.feed.hit_test(row) {
                                self.feed.state.select(Some(idx));
                            }
                        }
                    } else if layout.detail.contains((mouse.column, mouse.row).into()) {
                        self.focused = FocusedPanel::Detail;
                    } else if layout.log.contains((mouse.column, mouse.row).into()) {
                        self.focused = FocusedPanel::Log;
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Moved => {
                let layout = MainLayout::compute(
                    _size,
                    self.vsplit1.left_width,
                    self.vsplit2.left_width,
                    self.hsplit.bottom_height,
                );
                if self.vsplit1.is_dragging() {
                    // vsplit1's parent is the left pane (sidebar+vsplit1+feed),
                    // not the full terminal — using the full width let the
                    // splitter detach from the cursor mid-drag.
                    let parent_width = layout.sidebar.width + 1 + layout.feed.width;
                    self.vsplit1.on_mouse_move(mouse.column, parent_width);
                } else if self.vsplit2.is_dragging() {
                    self.vsplit2.on_mouse_move(mouse.column, _size.width);
                } else if self.hsplit.is_dragging() {
                    self.hsplit.on_mouse_move(mouse.row, _size.height);
                } else if matches!(mouse.kind, MouseEventKind::Moved) {
                    // Not dragging — just update hover feedback.
                    self.vsplit1.set_hover(near(mouse.column, layout.vsplit1.x));
                    self.vsplit2.set_hover(near(mouse.column, layout.vsplit2.x));
                    self.hsplit.set_hover(near(mouse.row, layout.hsplit.y));
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.vsplit1.on_mouse_up();
                self.vsplit2.on_mouse_up();
                self.hsplit.on_mouse_up();
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let layout = MainLayout::compute(
                    _size,
                    self.vsplit1.left_width,
                    self.vsplit2.left_width,
                    self.hsplit.bottom_height,
                );
                let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
                if layout.feed.contains((mouse.column, mouse.row).into()) {
                    if up {
                        self.feed.select_prev();
                    } else {
                        self.feed.select_next();
                    }
                } else if layout.log.contains((mouse.column, mouse.row).into()) {
                    if up {
                        self.log_state.scroll_up(3);
                    } else {
                        self.log_state.scroll_down(3);
                    }
                } else if layout.sidebar.contains((mouse.column, mouse.row).into()) {
                    if up {
                        self.sidebar.select_prev();
                    } else {
                        self.sidebar.select_next();
                    }
                    self.refresh_after_profile_change();
                }
            }
            _ => {}
        }
    }

    /// Satisfy a now-ready debounced gallery fetch: reuse the cached result
    /// for this listing if we've already scraped it (revisiting a listing
    /// never re-hits the network), otherwise enqueue a job on the gallery
    /// worker's own queue/thread. Returns true if served from cache.
    fn fire_or_serve_gallery(&mut self, pending: PendingGallery) -> bool {
        if let Some(images) = self.gallery_cache.get(&pending.listing_id).cloned() {
            if self.detail.current_listing().map(|l| l.id.as_str())
                == Some(pending.listing_id.as_str())
            {
                self.detail.set_gallery(images);
                self.refresh_hero_image(false);
            }
            true
        } else {
            let _ = self.gallery_tx.send(GalleryJob {
                listing_id: pending.listing_id,
                url: pending.url,
                source_id: pending.source_id,
            });
            false
        }
    }

    /// (Re)fetch the hero image for the detail panel's current image index,
    /// using the on-disk/memory cache when available and otherwise enqueuing
    /// a background download via the worker thread.
    /// `replace_immediately`: true for an actual listing/image-index change
    /// (the old hero is definitely stale, blank it now); false when
    /// swapping in a same-listing gallery replacement, where the old hero
    /// stays visible until the replacement resolves — gallery URLs are a
    /// different size variant than the feed thumbnail, so eagerly clearing
    /// here would blank every hero for the length of a network round trip.
    fn refresh_hero_image(&mut self, replace_immediately: bool) {
        self.pending_img_url = None;
        if replace_immediately {
            self.detail.hero_image_path = None;
        }

        let Some(url) = self.detail.current_image_url().map(str::to_string) else {
            self.detail.hero_image_path = None;
            return;
        };
        if let Some(path) = self.thumb_cache.check(&url) {
            self.detail.hero_image_path = Some(path);
            return;
        }
        self.pending_img_url = Some(url.clone());
        // Skip enqueue if a download for this URL is already in flight.
        if self.pending_thumb_urls.insert(url.clone()) {
            let _ = self.img_tx.send(url);
        }
    }

    fn selected_listing(&self) -> Option<&Listing> {
        self.feed.focused_listing()
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
        let active_profile_id = self.sidebar.selected_profile_id().map(str::to_string);
        if let Ok(listings) = dl.get_listings(active_profile_id.as_deref(), 100) {
            self.feed.update_listings(listings, true);
        }
        if let Ok(stats) = dl.get_profile_stats() {
            self.sidebar.update_stats(stats);
        }
        // Total only over profiles still present in config — an unfiltered
        // sum over the whole DB would keep counting a deleted profile's
        // orphaned rows forever.
        let known_ids: Vec<String> =
            self.sidebar.profiles().iter().map(|p| p.id.clone()).collect();
        if let Ok(filtered) = self.db.count_new_by_profile_filtered(&known_ids) {
            self.status_state.new_count = filtered.values().sum::<usize>() as u32;
        }
        if let Ok(states) = dl.get_source_states() {
            self.status_state.source_states = states;
        }
        if let Ok(Some(ts)) = dl.get_last_source_poll() {
            self.status_state.last_poll_iso = Some(ts);
        }

        // Check daemon status
        let (daemon_up, active_polls, daemon_profile_ids, bot_blocks, ai, polling) =
            self.check_daemon_status();
        self.status_state.daemon_up = daemon_up;
        self.status_state.active_polls = active_polls;
        self.status_state.bot_blocks = bot_blocks;
        self.status_state.ai = ai;
        // Resolve polling profile ids to display names (fall back to the id).
        self.status_state.polling = polling
            .into_iter()
            .map(|(pid, src)| {
                let name = self
                    .config
                    .profiles
                    .iter()
                    .find(|p| p.id == pid)
                    .map(|p| p.name.clone())
                    .unwrap_or(pid);
                (name, src)
            })
            .collect();
        self.sidebar.set_daemon_profiles(daemon_profile_ids);

        // Update poll interval from active profile
        if let Some(pid) = active_profile_id {
            if let Some(p) = self.sidebar.profiles().iter().find(|p| p.id == pid) {
                self.status_state.poll_interval_sec = p.poll_interval_sec as u32;
            }
        }
    }

    /// Returns (daemon_up, active poll source ids, profile ids the daemon
    /// currently knows about, (source_id, blocked url) pairs from bot_blocks).
    fn check_daemon_status(&self) -> DaemonStatusReport {
        let socket_path = self.config.socket_path();
        let Ok(mut stream) = UnixStream::connect(&socket_path) else {
            return (false, vec![], vec![], vec![], None, vec![]);
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .ok();
        let cmd = b"{\"command\":\"status\"}\n";
        if stream.write_all(cmd).is_err() {
            return (false, vec![], vec![], vec![], None, vec![]);
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
            return (false, vec![], vec![], vec![], None, vec![]);
        };
        if resp.get("status").and_then(|v| v.as_str()) != Some("ok") {
            return (false, vec![], vec![], vec![], None, vec![]);
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
        // STATUS-PROTOCOL CONTRACT: data.profiles is a flat array of enabled
        // profile-id strings, identical to the Python daemon — not objects.
        let daemon_profile_ids: Vec<String> = data
            .get("profiles")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        // data.bot_blocks is {plugin_id: blocked_url} — surfaced so the
        // status bar can flag it and 'x' can open it in a visible browser.
        let bot_blocks: Vec<(String, String)> = data
            .get("bot_blocks")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|u| (k.clone(), u.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let ai = data.get("ai").and_then(|v| v.as_object()).map(|obj| AiStatus {
            enabled: obj.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
            healthy: obj.get("healthy").and_then(|v| v.as_bool()).unwrap_or(true),
            detail: obj
                .get("detail")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        });
        let polling: Vec<(String, String)> = data
            .get("polling")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        let p = v.get("profile")?.as_str()?.to_string();
                        let src = v.get("source")?.as_str()?.to_string();
                        Some((p, src))
                    })
                    .collect()
            })
            .unwrap_or_default();
        (true, active, daemon_profile_ids, bot_blocks, ai, polling)
    }

    /// Fire-and-forget: write a command and don't wait for a response.
    /// Used only where there is nothing meaningful left to do with the
    /// reply (e.g. the shutdown command sent on the way out the door).
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

    /// Send a command and parse the daemon's JSON response, mirroring
    /// `check_daemon_status`'s read loop. `Err` means the daemon couldn't
    /// be reached or its response couldn't be parsed — never an
    /// application-level rejection, which callers read from the returned
    /// `status`/`message` fields instead.
    fn send_daemon_command_checked(&self, json_line: &str) -> Result<serde_json::Value, String> {
        let socket_path = self.config.socket_path();
        let mut stream = UnixStream::connect(&socket_path).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

        let mut msg = json_line.as_bytes().to_vec();
        if !msg.ends_with(b"\n") {
            msg.push(b'\n');
        }
        stream.write_all(&msg).map_err(|e| e.to_string())?;

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
                Err(e) => return Err(e.to_string()),
            }
        }
        serde_json::from_slice(&buf).map_err(|e| e.to_string())
    }
}

/// Centered modal listing every keybinding — opened by '?', closed by any
/// key. Every binding advertised elsewhere only in a terse status-bar hint
/// or an unlabeled chevron gets a real explanation here.
fn render_help_overlay(area: Rect, buf: &mut Buffer) {
    const BINDINGS: &[(&str, &str)] = &[
        ("Tab / Shift+Tab", "switch panel focus"),
        ("\u{2190} / \u{2192}", "switch panel focus"),
        ("j/k or \u{2191}/\u{2193}", "move selection"),
        ("Enter", "mark listing seen"),
        ("o", "open listing in browser"),
        ("s", "save listing"),
        ("d", "dismiss listing"),
        ("n", "snooze listing 24h"),
        (", / .", "prev / next image"),
        ("S", "cycle sort order"),
        ("a", "add profile"),
        ("e", "edit profile"),
        ("r", "repoll active profile"),
        ("x", "open bot-blocked listing(s)"),
        ("q", "quit"),
        ("Q", "quit and stop daemon"),
        ("?", "toggle this help"),
    ];

    let width = 46u16.min(area.width.saturating_sub(2));
    let height = (BINDINGS.len() as u16 + 2).min(area.height.saturating_sub(2));
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
        .border_style(Style::default().fg(colors::ORANGE))
        .style(Style::default().bg(colors::BG_DETAIL))
        .title(Span::styled(
            " KEYS ",
            Style::default()
                .fg(colors::ORANGE)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(overlay);
    block.render(overlay, buf);

    let lines: Vec<Line> = BINDINGS
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(format!("{key:<16}"), Style::default().fg(colors::ORANGE)),
                Span::styled(*desc, Style::default().fg(colors::TEXT_MUTED)),
            ])
        })
        .collect();

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(inner, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_allows_one_column_tolerance() {
        assert!(near(10, 10));
        assert!(near(9, 10));
        assert!(near(11, 10));
        assert!(!near(8, 10));
        assert!(!near(12, 10));
    }

    #[test]
    fn pending_gallery_not_ready_before_debounce() {
        let pending = PendingGallery {
            listing_id: "a".to_string(),
            url: "https://example.com/a".to_string(),
            source_id: "ebay".to_string(),
            requested_at: Instant::now(),
        };
        assert!(!pending.is_ready(GALLERY_DEBOUNCE));
    }

    #[test]
    fn pending_gallery_ready_after_debounce() {
        let pending = PendingGallery {
            listing_id: "a".to_string(),
            url: "https://example.com/a".to_string(),
            source_id: "ebay".to_string(),
            requested_at: Instant::now() - Duration::from_millis(2000),
        };
        assert!(pending.is_ready(GALLERY_DEBOUNCE));
    }

    fn write_test_config(dir: &std::path::Path) -> PathBuf {
        let config_path = dir.join("config.toml");
        let toml = format!(
            "[global]\ndb_path = \"{}\"\nsocket_path = \"{}\"\n",
            dir.join("scavenger.db").display(),
            dir.join("daemon.sock").display(),
        );
        std::fs::write(&config_path, toml).expect("write test config");
        config_path
    }

    fn make_app(dir: &std::path::Path) -> App {
        let config_path = write_test_config(dir);
        let config = config::load_config(&config_path).expect("load test config");
        App::new(config, config_path).expect("construct App against temp config/db")
    }

    fn sample_listing(id: &str) -> Listing {
        Listing {
            id: id.to_string(),
            profile_id: "p1".to_string(),
            source_id: "ebay".to_string(),
            title: "widget".to_string(),
            description: String::new(),
            price: None,
            currency: "USD".to_string(),
            condition: None,
            url: format!("https://example.com/{id}"),
            image_urls: vec!["feed-thumb.jpg".to_string()],
            location: None,
            first_seen: chrono::Utc::now(),
            last_seen: chrono::Utc::now(),
            relevance_score: 0.0,
            status: crate::models::ListingStatus::New,
            ai_evaluation: None,
        }
    }

    #[test]
    fn fire_or_serve_gallery_reuses_cached_result() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut app = make_app(dir.path());
        app.detail.show_listing(Some(sample_listing("a")));
        app.gallery_cache
            .insert("a".to_string(), vec!["full1.jpg".to_string(), "full2.jpg".to_string()]);

        let pending = PendingGallery {
            listing_id: "a".to_string(),
            url: "https://example.com/a".to_string(),
            source_id: "ebay".to_string(),
            requested_at: Instant::now(),
        };

        let served_from_cache = app.fire_or_serve_gallery(pending);

        assert!(served_from_cache, "cached listing should not hit the network");
        assert_eq!(app.detail.image_count(), 2);
        assert_eq!(app.detail.current_image_url(), Some("full1.jpg"));
    }

    #[test]
    fn fire_or_serve_gallery_misses_cache_for_unscraped_listing() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut app = make_app(dir.path());
        app.detail.show_listing(Some(sample_listing("b")));

        let pending = PendingGallery {
            listing_id: "b".to_string(),
            url: "https://example.com/b".to_string(),
            source_id: "ebay".to_string(),
            requested_at: Instant::now(),
        };

        let served_from_cache = app.fire_or_serve_gallery(pending);

        assert!(!served_from_cache);
        // Cache miss doesn't touch the detail panel's images synchronously
        // — only an actual ImgResult::Gallery arrival does that.
        assert_eq!(app.detail.image_count(), 1);
    }

    fn sample_form_data(id: &str, name: &str) -> ProfileFormData {
        ProfileFormData {
            id: id.to_string(),
            name: name.to_string(),
            keywords: vec![crate::models::KeywordGroup::Single("bike".into())],
            sources: vec!["ebay".to_string()],
            negative_keywords: vec![],
            price_min: None,
            price_max: None,
            poll_interval_sec: 3600,
            alert_priority: "normal".to_string(),
            tags: vec![],
            escalation_keywords: vec![],
            location_radius_mi: None,
        }
    }

    /// The daemon socket in these tests always points at a path nothing is
    /// listening on, exercising the "daemon unreachable" branch of
    /// `finish_crud_write` — non-fatal, since the config write to disk
    /// already succeeded.
    #[test]
    fn crud_flow_updates_disk_config_and_in_memory_state_with_unreachable_daemon() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut app = make_app(dir.path());

        // Create
        app.apply_form_result(FormResult::Create(sample_form_data(
            "mountain-bikes",
            "Mountain Bikes",
        )));
        assert!(matches!(app.screen, Screen::Main));
        assert_eq!(app.config.profiles.len(), 1);
        assert_eq!(app.config.profiles[0].name, "Mountain Bikes");
        assert_eq!(app.sidebar.profiles().len(), 1);
        let on_disk = config::load_config(&app.config_path).unwrap();
        assert_eq!(on_disk.profiles.len(), 1);
        assert_eq!(on_disk.profiles[0].id, "mountain-bikes");

        // Update
        app.apply_form_result(FormResult::Update(sample_form_data(
            "mountain-bikes",
            "Road Bikes",
        )));
        assert!(matches!(app.screen, Screen::Main));
        assert_eq!(app.config.profiles.len(), 1);
        assert_eq!(app.config.profiles[0].name, "Road Bikes");
        assert_eq!(app.sidebar.profiles()[0].name, "Road Bikes");
        let on_disk = config::load_config(&app.config_path).unwrap();
        assert_eq!(on_disk.profiles[0].name, "Road Bikes");

        // Delete
        app.apply_form_result(FormResult::Delete {
            id: "mountain-bikes".to_string(),
        });
        assert!(matches!(app.screen, Screen::Main));
        assert!(app.config.profiles.is_empty());
        assert!(app.sidebar.profiles().is_empty());
        let on_disk = config::load_config(&app.config_path).unwrap();
        assert!(on_disk.profiles.is_empty());
    }

    #[test]
    fn create_with_duplicate_id_keeps_modal_open_and_state_unchanged() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut app = make_app(dir.path());

        app.apply_form_result(FormResult::Create(sample_form_data("bikes", "Bikes")));
        assert!(matches!(app.screen, Screen::Main));

        app.screen = Screen::ProfileForm(Box::new(ProfileForm::new(None)));
        app.apply_form_result(FormResult::Create(sample_form_data("bikes", "Bikes Again")));

        match &app.screen {
            Screen::ProfileForm(form) => assert!(form.error_msg.is_some()),
            _ => panic!("CRUD error should keep the modal open"),
        }
        // Rejected write never touched in-memory state.
        assert_eq!(app.config.profiles.len(), 1);
        assert_eq!(app.config.profiles[0].name, "Bikes");
    }

    /// A daemon that actively rejects the reload (status != "ok") must
    /// keep the modal open with its message — distinct from the
    /// unreachable-daemon case above, which closes the modal since the
    /// config write itself already succeeded.
    #[test]
    fn daemon_rejection_keeps_modal_open_with_daemon_message() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = write_test_config(dir.path());
        let config = config::load_config(&config_path).unwrap();
        let socket_path = config.socket_path();

        let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        // The CRUD flow makes two round trips before this returns: a
        // status check (from `reload_profiles`'s `poll_data`) and the
        // actual reload. Serve status benignly and only reject the reload.
        let server = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 256];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                if req.contains("\"reload\"") {
                    let _ = stream.write_all(b"{\"status\":\"error\",\"message\":\"bad profile\"}\n");
                    break;
                }
                let _ = stream.write_all(b"{\"status\":\"ok\",\"data\":{}}\n");
            }
        });

        let mut app = App::new(config, config_path).expect("construct App");
        app.screen = Screen::ProfileForm(Box::new(ProfileForm::new(None)));
        app.apply_form_result(FormResult::Create(sample_form_data("bikes", "Bikes")));
        server.join().unwrap();

        match &app.screen {
            Screen::ProfileForm(form) => {
                assert_eq!(form.error_msg.as_deref(), Some("bad profile"));
            }
            _ => panic!("daemon rejection should keep the modal open"),
        }
        // The config write to disk already succeeded before the daemon
        // was consulted, so in-memory state still reflects it.
        assert_eq!(app.config.profiles.len(), 1);
    }
}
