"""Shared Chrome CDP connection for scraping plugins.

Requires Chrome running with remote debugging enabled:
    google-chrome-stable --remote-debugging-port=9222 --user-data-dir=/tmp/scavenger-chrome &

No fallback — if Chrome isn't available on port 9222, scraping fails loudly.
"""
import asyncio
import logging
from playwright.async_api import async_playwright, Browser, Playwright, BrowserContext, Page

logger = logging.getLogger(__name__)

_browser: Browser | None = None
_playwright: Playwright | None = None
_lock = asyncio.Lock()

CDP_URL = "http://localhost:9222"

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
    """Return a CDP-connected Browser. Raises if Chrome isn't on port 9222."""
    global _browser, _playwright
    async with _lock:
        if _browser is not None and _browser.is_connected():
            return _browser

        if _playwright is None:
            _playwright = await async_playwright().start()

        _browser = await _playwright.chromium.connect_over_cdp(CDP_URL, timeout=5000)
        logger.info("Connected to Chrome via CDP at %s", CDP_URL)
    return _browser


async def new_page() -> tuple[BrowserContext, Page]:
    """Return (context, page). Caller MUST close both when done."""
    browser = await get_browser()
    context = await browser.new_context(**CONTEXT_OPTIONS)
    page = await context.new_page()
    return context, page
