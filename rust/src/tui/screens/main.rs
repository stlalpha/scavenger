use std::path::{Path, PathBuf};

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Widget;

use crate::tui::widgets::detail_panel::DetailPanel;
use crate::tui::widgets::log_panel::{LogPanelState, LogPanelWidget};
use crate::tui::widgets::profile_sidebar::ProfileSidebar;
use crate::tui::widgets::results_feed::ResultsFeed;
use crate::tui::widgets::splitter::{
    HSplitterState, HSplitterWidget, VSplitterState, VSplitterWidget,
};
use crate::tui::widgets::status_bar::{StatusBarState, StatusBarWidget};
use crate::tui::FocusedPanel;

/// Compute the main layout areas.
///
/// ```text
/// ┌──────────┬─────────────────┬──────────────────┐
/// │ PROFILES │    LISTINGS     │     DETAIL       │
/// │ (sidebar)│    (feed)       │    (panel)       │
/// ├──────────┴─────────────────┴──────────────────┤
/// │                    LOG (variable)              │
/// ├────────────────────────────────────────────────┤
/// │                 STATUS BAR (1 row)             │
/// └────────────────────────────────────────────────┘
/// ```
pub struct MainLayout {
    pub sidebar: Rect,
    pub vsplit1: Rect,
    pub feed: Rect,
    pub vsplit2: Rect,
    pub detail: Rect,
    pub hsplit: Rect,
    pub log: Rect,
    pub status_bar: Rect,
}

impl MainLayout {
    pub fn compute(
        area: Rect,
        sidebar_width: u16,
        vsplit2_left_width: u16,
        log_height: u16,
    ) -> Self {
        // Top-level vertical: [content, hsplit(1), log, status_bar(1)]
        let vert = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),           // content area
                Constraint::Length(1),         // horizontal splitter
                Constraint::Length(log_height),// log panel
                Constraint::Length(1),         // status bar
            ])
            .split(area);

        let content_area = vert[0];
        let hsplit_area = vert[1];
        let log_area = vert[2];
        let status_area = vert[3];

        // Horizontal split of content: [sidebar, vsplit(1), feed, vsplit(1), detail]
        // `vsplit2_left_width` is the combined width of sidebar+vsplit1+feed,
        // as tracked by VSplitterState and updated by dragging. 0 is the
        // "never dragged" sentinel — fall back to the original 60/40 default.
        let left_total = content_area.width.saturating_sub(1); // minus 1 for vsplit2
        let left_width = if vsplit2_left_width == 0 {
            left_total.saturating_sub(left_total * 40 / 100)
        } else {
            vsplit2_left_width.min(left_total)
        };

        let horiz = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(left_width),
                Constraint::Length(1),       // vsplit2
                Constraint::Min(20),         // detail
            ])
            .split(content_area);

        let left_area = horiz[0];
        let vsplit2_area = horiz[1];
        let detail_area = horiz[2];

        // Split left area into [sidebar, vsplit1(1), feed]
        let left_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(sidebar_width),
                Constraint::Length(1),  // vsplit1
                Constraint::Min(20),   // feed
            ])
            .split(left_area);

        Self {
            sidebar: left_cols[0],
            vsplit1: left_cols[1],
            feed: left_cols[2],
            vsplit2: vsplit2_area,
            detail: detail_area,
            hsplit: hsplit_area,
            log: log_area,
            status_bar: status_area,
        }
    }
}

/// Caches the decoded image and ratatui_image protocol for the detail
/// panel's hero image, so the draw loop (~10x/sec) only re-decodes on
/// selection change or resize instead of every frame.
pub struct HeroImageCache {
    picker: ratatui_image::picker::Picker,
    path: Option<PathBuf>,
    area: Rect,
    protocol: Option<ratatui_image::protocol::Protocol>,
    /// The last (path, area, fs-signature) that failed to decode — skipped
    /// on subsequent frames instead of retrying a doomed decode ~10x/sec.
    /// The fs-signature (mtime, len) is part of the key so that when a
    /// still-downloading file finishes at the same path, the changed file
    /// no longer matches the poisoned entry and gets decoded.
    failed: Option<FailedDecode>,
}

/// (path, target area, file fs-signature) identifying a failed decode.
type FailedDecode = (PathBuf, Rect, Option<(u64, u64)>);

/// (mtime-secs, len) for a file, or None if it can't be stat'd. Used to
/// tell "the same failed file" from "the file changed since it failed".
fn fs_signature(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((mtime, m.len()))
}

impl HeroImageCache {
    pub fn new() -> Self {
        Self {
            // Halfblocks (unicode ▀▄) — lowest-common-denominator fallback.
            // detect_terminal() upgrades this to the terminal's real
            // graphics protocol once the terminal is in raw mode.
            picker: ratatui_image::picker::Picker::from_fontsize((8, 12)),
            path: None,
            area: Rect::default(),
            protocol: None,
            failed: None,
        }
    }

    /// Query the terminal for its graphics protocol (Kitty/iTerm2/Sixel)
    /// and real cell size, so images render at full resolution instead of
    /// halfblock mosaics. Must run while the terminal is in raw mode and
    /// before the first draw; keeps the halfblock fallback on failure.
    pub fn detect_terminal(&mut self) {
        if let Ok(picker) = ratatui_image::picker::Picker::from_query_stdio() {
            self.picker = picker;
            // Invalidate anything encoded with the fallback picker.
            self.protocol = None;
            self.path = None;
            self.failed = None;
        }
    }

    fn ensure(&mut self, img_path: &Path, area: Rect) {
        if self.protocol.is_some() && self.path.as_deref() == Some(img_path) && self.area == area
        {
            return;
        }
        let sig = fs_signature(img_path);
        if self
            .failed
            .as_ref()
            .is_some_and(|(p, a, s)| p.as_path() == img_path && *a == area && *s == sig)
        {
            return;
        }
        self.path = None;
        self.protocol = None;

        let Ok(dyn_img) = image::ImageReader::open(img_path)
            .and_then(|r| r.with_guessed_format())
            .map_err(|e| e.to_string())
            .and_then(|r| r.decode().map_err(|e| e.to_string()))
        else {
            self.failed = Some((img_path.to_path_buf(), area, sig));
            return;
        };

        match self
            .picker
            .new_protocol(dyn_img, area, ratatui_image::Resize::Fit(None))
        {
            Ok(proto) => {
                self.protocol = Some(proto);
                self.path = Some(img_path.to_path_buf());
                self.area = area;
                self.failed = None;
            }
            Err(_) => {
                self.failed = Some((img_path.to_path_buf(), area, sig));
            }
        }
    }
}

impl Default for HeroImageCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Render the full main screen into the buffer.
#[allow(clippy::too_many_arguments)]
pub fn render_main_screen(
    area: Rect,
    buf: &mut Buffer,
    sidebar: &mut ProfileSidebar,
    feed: &mut ResultsFeed,
    detail: &DetailPanel,
    focused: FocusedPanel,
    vsplit1: &VSplitterState,
    vsplit2: &VSplitterState,
    hsplit: &HSplitterState,
    log_state: &LogPanelState,
    status_state: &StatusBarState,
    image_path: Option<&Path>,
    hero_cache: &mut HeroImageCache,
) {
    let layout = MainLayout::compute(
        area,
        vsplit1.left_width,
        vsplit2.left_width,
        hsplit.bottom_height,
    );

    sidebar.render(layout.sidebar, buf, focused == FocusedPanel::Profiles);

    // -- Splitters --
    VSplitterWidget::new(vsplit1).render(layout.vsplit1, buf);
    VSplitterWidget::new(vsplit2).render(layout.vsplit2, buf);
    HSplitterWidget::new(hsplit).render(layout.hsplit, buf);

    feed.render(layout.feed, buf, focused == FocusedPanel::Feed);

    // Detail panel draws its own text/placeholder and hands back the Rect
    // reserved for the hero image, if any — actual image compositing needs
    // a live ratatui_image Picker, which stays here rather than in the
    // widget layer.
    if let Some(image_area) = detail.render(layout.detail, buf, focused == FocusedPanel::Detail) {
        if let Some(img_path) = image_path {
            render_hero_image(hero_cache, img_path, image_area, buf);
        }
    }

    // -- Log --
    LogPanelWidget::new(log_state, focused == FocusedPanel::Log).render(layout.log, buf);

    // -- Status bar --
    StatusBarWidget::new(status_state).render(layout.status_bar, buf);
}

fn render_hero_image(cache: &mut HeroImageCache, img_path: &Path, area: Rect, buf: &mut Buffer) {
    cache.ensure(img_path, area);
    if let Some(proto) = &cache.protocol {
        let img = ratatui_image::Image::new(proto);
        Widget::render(img, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hero_image_cache_records_and_skips_failed_decode() {
        let mut cache = HeroImageCache::new();
        let dir = tempfile::TempDir::new().unwrap();
        let bad_path = dir.path().join("not-an-image.jpg");
        std::fs::write(&bad_path, b"not an image").unwrap();

        let area = Rect::new(0, 0, 20, 10);
        cache.ensure(&bad_path, area);
        assert!(cache.protocol.is_none());
        let sig_v1 = cache.failed.clone();
        assert!(matches!(&sig_v1, Some((p, a, _)) if *p == bad_path && *a == area));

        // Unchanged file at the same (path, area): short-circuits — the
        // recorded failure signature is identical, so no re-decode.
        cache.ensure(&bad_path, area);
        assert_eq!(cache.failed, sig_v1);

        // The file changed at the same path (a download finishing is the
        // real case). The fs-signature differs, so the poisoned entry no
        // longer matches and `ensure` retries — proven by the recorded
        // signature updating to the new file.
        std::fs::write(&bad_path, b"still not an image but a different length").unwrap();
        cache.ensure(&bad_path, area);
        assert!(cache.failed.is_some());
        assert_ne!(cache.failed, sig_v1, "changed file must be retried, not permanently poisoned");

        // A different area also invalidates the entry and retries.
        let other_area = Rect::new(0, 0, 20, 20);
        cache.ensure(&bad_path, other_area);
        assert!(matches!(&cache.failed, Some((_, a, _)) if *a == other_area));
    }
}
