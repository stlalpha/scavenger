//! Shared Chrome CDP connection for scraping plugins.
//!
//! Requires Chrome running with remote debugging enabled:
//!     google-chrome-stable --remote-debugging-port=9222 --user-data-dir="$HOME/Library/Application Support/scavenger/chrome" &
//!
//! Uses the real Chrome session (cookies, fingerprint, history) to avoid bot
//! detection. Pages open as real tabs in Chrome. Close the page when done;
//! never close the browser context.

use std::sync::Arc;

use anyhow::{Context, Result};
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams;
use chromiumoxide::Page;
use futures::StreamExt;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

const CDP_URL: &str = "http://localhost:9222";

/// Timeout for connecting to Chrome CDP.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Timeout for creating a new page tab.
const NEW_PAGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Stealth JS injected into every new page via `Page.addScriptToEvaluateOnNewDocument`.
/// Removes the `navigator.webdriver` flag that bot-detection scripts look for.
const STEALTH_JS: &str = r#"
    Object.defineProperty(navigator, 'webdriver', {
        get: () => false,
    });
"#;

/// Global shared browser connection. Uses Mutex<Option<>> so we can retry after failure.
static BROWSER: std::sync::LazyLock<Mutex<Option<Arc<SharedBrowser>>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

/// Holds the browser handle and its event-loop task.
struct SharedBrowser {
    browser: Browser,
    /// The browser event handler runs in the background. We hold onto the
    /// JoinHandle so the task isn't dropped, but we never need to await it.
    _handler: tokio::task::JoinHandle<()>,
}

/// Get or create the global CDP browser connection. Retries if previous attempt failed.
async fn get_browser() -> Result<Arc<SharedBrowser>> {
    let mut guard = BROWSER.lock().await;
    if let Some(ref shared) = *guard {
        return Ok(Arc::clone(shared));
    }
    let shared = init_browser().await?;
    *guard = Some(Arc::clone(&shared));
    Ok(shared)
}

/// Clear the cached browser connection (e.g. after Chrome restarts), but
/// only if it's still the handle that failed -- a concurrent caller may
/// have already installed a fresh one.
async fn clear_browser_if(failed: &Arc<SharedBrowser>) {
    let mut guard = BROWSER.lock().await;
    if let Some(ref cached) = *guard {
        if Arc::ptr_eq(cached, failed) {
            *guard = None;
            debug!("Cleared cached browser connection");
        }
    }
}

/// Clear the cached browser connection unconditionally (e.g. daemon shutdown).
pub async fn clear_browser() {
    let mut guard = BROWSER.lock().await;
    *guard = None;
    debug!("Cleared cached browser connection");
}

/// Timeout for the warmup open/close during browser init.
const WARMUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

async fn init_browser() -> Result<Arc<SharedBrowser>> {
    let (browser, mut handler) = tokio::time::timeout(CONNECT_TIMEOUT, Browser::connect(CDP_URL))
        .await
        .map_err(|_| anyhow::anyhow!("Chrome CDP connection timed out after {}s", CONNECT_TIMEOUT.as_secs()))?
        .context("Failed to connect to Chrome CDP. Is Chrome running with --remote-debugging-port=9222?")?;

    // Spawn the CDP event handler as a background task.
    let handle = tokio::spawn(async move {
        while let Some(_event) = handler.next().await {}
    });

    info!("Connected to Chrome via CDP at {}", CDP_URL);

    // Warm up: open about:blank and close it to prime the network stack.
    tokio::time::timeout(WARMUP_TIMEOUT, async {
        let warmup = browser.new_page("about:blank").await?;
        warmup.close().await?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|_| anyhow::anyhow!("Chrome warmup timed out after {}s", WARMUP_TIMEOUT.as_secs()))??;
    debug!("Chrome network stack ready");

    Ok(Arc::new(SharedBrowser {
        browser,
        _handler: handle,
    }))
}

/// Create a new tab in the user's Chrome session.
///
/// The page has stealth JS injected (hides `navigator.webdriver`).
/// Caller MUST close the page when done:
///
/// ```rust,no_run
/// # async fn example() -> anyhow::Result<()> {
/// let page = scavenger::plugins::browser::new_page().await?;
/// // ... use page ...
/// page.close().await?;
/// # Ok(())
/// # }
/// ```
///
/// Never close the browser context -- it's the user's live Chrome session.
///
/// Self-healing: if the cached browser handle is stale (e.g. Chrome was
/// restarted), page creation fails against it, the cache is dropped, and
/// connect+create is retried exactly once against a fresh connection.
pub async fn new_page() -> Result<Page> {
    let shared = get_browser().await?;
    match try_new_page(Arc::clone(&shared)).await {
        Ok(page) => Ok(page),
        Err(e) => {
            warn!("new_page failed on cached browser, reconnecting: {e}");
            clear_browser_if(&shared).await;
            let fresh = get_browser().await?;
            try_new_page(fresh).await
        }
    }
}

async fn try_new_page(shared: Arc<SharedBrowser>) -> Result<Page> {
    let page = tokio::time::timeout(NEW_PAGE_TIMEOUT, async {
        let page = shared.browser.new_page("about:blank").await?;
        page.execute(AddScriptToEvaluateOnNewDocumentParams::new(STEALTH_JS))
            .await
            .context("Failed to inject stealth JS")?;
        Ok::<Page, anyhow::Error>(page)
    })
    .await
    .map_err(|_| anyhow::anyhow!("new_page timed out after {}s", NEW_PAGE_TIMEOUT.as_secs()))??;

    Ok(page)
}

// ---------------------------------------------------------------------------
// Common CDP operations for scraper plugins (reference guide)
// ---------------------------------------------------------------------------
//
// Navigation:
//   page.goto("https://example.com").await?;
//
// Find elements:
//   let el = page.find_element("css-selector").await?;
//   let els = page.find_elements("css-selector").await?;
//
// Extract text from an element:
//   let text: String = el.inner_text().await?.unwrap_or_default();
//
// Get an attribute (href, src, data-src, etc.):
//   let href: Option<String> = el.attribute("href").await?;
//
// Evaluate JavaScript (e.g. scroll to bottom):
//   page.evaluate_expression("window.scrollTo(0, document.body.scrollHeight)")
//       .await?;
//
// Wait for a selector to appear (with implicit timeout):
//   page.find_element("css-selector").await?;
//   // chromiumoxide's find_element waits for the element by default.
//   // For explicit timeout control, use page.wait_for_navigation().await
//   // combined with a timeout wrapper: tokio::time::timeout(dur, ...).await
//
// Get full page HTML:
//   let html: String = page.content().await?;
//
// Close the page (always do this in a finally/drop guard):
//   page.close().await?;

/// RAII guard that closes a CDP page on drop, preventing tab leaks.
///
/// Access the inner `Page` via `guard.page`. The page is closed automatically
/// when the guard is dropped, even on early return or panic.
pub struct PageGuard {
    page: Option<Page>,
}

impl PageGuard {
    pub fn new(page: Page) -> Self {
        Self { page: Some(page) }
    }

    /// Borrow the inner page. Fails if the page has already been taken by
    /// `close()` -- callers are expected to use `?` to propagate rather than
    /// treat this as unreachable.
    pub fn page(&self) -> Result<&Page, crate::plugins::PluginError> {
        self.page
            .as_ref()
            .ok_or_else(|| crate::plugins::PluginError::Other("page already taken".into()))
    }

    /// Explicitly close the page and consume the guard.
    pub async fn close(mut self) -> Result<()> {
        if let Some(page) = self.page.take() {
            page.close().await.context("failed to close page")?;
        }
        Ok(())
    }
}

impl Drop for PageGuard {
    fn drop(&mut self) {
        if let Some(page) = self.page.take() {
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    handle.spawn(async move {
                        if let Err(e) = page.close().await {
                            warn!("PageGuard drop: failed to close page: {e}");
                        }
                    });
                }
                Err(e) => {
                    warn!("PageGuard drop: no tokio runtime, leaking tab: {e}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stealth_js_hides_webdriver() {
        assert!(STEALTH_JS.contains("webdriver"));
        assert!(STEALTH_JS.contains("false"));
    }

    #[test]
    fn cdp_url_is_localhost_9222() {
        assert_eq!(CDP_URL, "http://localhost:9222");
    }
}
