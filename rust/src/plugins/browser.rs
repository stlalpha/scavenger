//! Shared Chrome CDP connection for scraping plugins.
//!
//! Requires Chrome running with remote debugging enabled:
//!     google-chrome-stable --remote-debugging-port=9222 --user-data-dir=/tmp/scavenger-chrome &
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
use tokio::sync::OnceCell;
use tracing::{debug, info};

const CDP_URL: &str = "http://localhost:9222";

/// Stealth JS injected into every new page via `Page.addScriptToEvaluateOnNewDocument`.
/// Removes the `navigator.webdriver` flag that bot-detection scripts look for.
const STEALTH_JS: &str = r#"
    Object.defineProperty(navigator, 'webdriver', {
        get: () => false,
    });
"#;

/// Global shared browser connection. Initialized once on first access.
static BROWSER: OnceCell<Arc<SharedBrowser>> = OnceCell::const_new();

/// Holds the browser handle and its event-loop task.
struct SharedBrowser {
    browser: Browser,
    /// The browser event handler runs in the background. We hold onto the
    /// JoinHandle so the task isn't dropped, but we never need to await it.
    _handler: tokio::task::JoinHandle<()>,
}

/// Get or create the global CDP browser connection.
async fn get_browser() -> Result<Arc<SharedBrowser>> {
    let shared = BROWSER
        .get_or_try_init(|| async { init_browser().await })
        .await?;
    Ok(Arc::clone(shared))
}

async fn init_browser() -> Result<Arc<SharedBrowser>> {
    let (browser, mut handler) = Browser::connect(CDP_URL)
        .await
        .context("Failed to connect to Chrome CDP. Is Chrome running with --remote-debugging-port=9222?")?;

    // Spawn the CDP event handler as a background task.
    let handle = tokio::spawn(async move {
        while let Some(_event) = handler.next().await {}
    });

    info!("Connected to Chrome via CDP at {}", CDP_URL);

    // Warm up: open about:blank and close it to prime the network stack.
    let warmup = browser.new_page("about:blank").await?;
    warmup.close().await?;
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
pub async fn new_page() -> Result<Page> {
    let shared = get_browser().await?;
    let page = shared.browser.new_page("about:blank").await?;

    // Inject stealth script before any navigation occurs.
    page.execute(AddScriptToEvaluateOnNewDocumentParams::new(STEALTH_JS))
        .await
        .context("Failed to inject stealth JS")?;

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
