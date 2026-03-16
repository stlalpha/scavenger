"""Browser connection for scraping plugins.

Connects to your running Chrome instance via CDP (preferred) so scraping
uses your real browser profile, cookies, and fingerprint — invisible to
bot detection.

To enable: launch Chrome once with:
    google-chrome-stable --remote-debugging-port=9222 &

If Chrome isn't available on port 9222, falls back to launching system Chrome.
"""
import asyncio
import logging
from playwright.async_api import async_playwright, Browser, Playwright, BrowserContext

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


async def get_browser() -> Browser:
    """Return a browser — CDP-connected if available, system Chrome otherwise."""
    global _browser, _playwright
    async with _lock:
        if _browser is not None and _browser.is_connected():
            return _browser

        if _playwright is None:
            _playwright = await async_playwright().start()

        # Try connecting to running Chrome first
        try:
            _browser = await _playwright.chromium.connect_over_cdp(CDP_URL, timeout=2000)
            logger.info("Connected to running Chrome via CDP at %s", CDP_URL)
            return _browser
        except Exception:
            logger.info("Chrome not on %s — launching system Chrome", CDP_URL)

        # Fall back to system Chrome binary
        _browser = await _playwright.chromium.launch(
            headless=False,
            executable_path=SYSTEM_CHROME,
            args=LAUNCH_ARGS,
        )
        logger.info("Launched system Chrome: %s", SYSTEM_CHROME)
    return _browser


async def new_context() -> BrowserContext:
    """Return a browser context.

    When connected via CDP, reuses the existing default context (preserving
    cookies/session). When launching fresh, creates a new context.
    """
    browser = await get_browser()

    # CDP-connected browsers expose the existing context directly
    if browser.contexts:
        return browser.contexts[0]

    return await browser.new_context(
        user_agent=(
            "Mozilla/5.0 (X11; Linux x86_64) "
            "AppleWebKit/537.36 (KHTML, like Gecko) "
            "Chrome/124.0.0.0 Safari/537.36"
        ),
        viewport={"width": 1280, "height": 900},
    )
