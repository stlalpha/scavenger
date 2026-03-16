"""Shared headless Chromium browser instance for all plugins.

Connects to your running Chrome instance via CDP (preferred) so scraping
uses your real browser fingerprint — invisible to bot detection.

To enable CDP mode, launch Chrome once with:
    google-chrome-stable --remote-debugging-port=9222 &

Falls back to launching system Chrome if CDP not available.
Each scrape gets its own fresh incognito context to prevent cross-contamination.
"""
import asyncio
import logging
from playwright.async_api import async_playwright, Browser, Playwright, BrowserContext, Page

logger = logging.getLogger(__name__)

_browser: Browser | None = None
_playwright: Playwright | None = None
_lock = asyncio.Lock()

CDP_URL = "http://localhost:9222"
SYSTEM_CHROME = "/usr/bin/google-chrome-stable"

LAUNCH_ARGS = [
    "--disable-blink-features=AutomationControlled",
    "--no-sandbox",
    "--disable-dev-shm-usage",
]

CONTEXT_OPTIONS = dict(
    user_agent=(
        "Mozilla/5.0 (X11; Linux x86_64) "
        "AppleWebKit/537.36 (KHTML, like Gecko) "
        "Chrome/124.0.0.0 Safari/537.36"
    ),
    viewport={"width": 1280, "height": 900},
    extra_http_headers={"Accept-Language": "en-US,en;q=0.9"},
)


async def get_browser() -> Browser:
    """Return a browser — CDP-connected if available, system Chrome otherwise."""
    global _browser, _playwright
    async with _lock:
        if _browser is not None and _browser.is_connected():
            return _browser

        if _playwright is None:
            _playwright = await async_playwright().start()

        try:
            _browser = await _playwright.chromium.connect_over_cdp(CDP_URL, timeout=5000)
            logger.info("Connected to running Chrome via CDP at %s", CDP_URL)
        except Exception as cdp_err:
            logger.warning("CDP connect failed (%s: %s) — launching system Chrome", type(cdp_err).__name__, cdp_err)
            _browser = await _playwright.chromium.launch(
                headless=False,
                executable_path=SYSTEM_CHROME,
                args=LAUNCH_ARGS,
            )
            logger.info("Launched system Chrome")
    return _browser


async def new_page() -> tuple[BrowserContext, Page]:
    """Return (context, page) with a fresh incognito context.

    Caller MUST close both when done:
        finally:
            await page.close()
            await context.close()
    """
    browser = await get_browser()
    context = await browser.new_context(**CONTEXT_OPTIONS)
    page = await context.new_page()
    return context, page
