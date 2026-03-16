"""Shared Chrome CDP connection for scraping plugins.

Requires Chrome running with remote debugging enabled:
    google-chrome-stable --remote-debugging-port=9222 --user-data-dir=/tmp/scavenger-chrome &

Uses the real Chrome session (cookies, fingerprint, history) — invisible to bot detection.
Pages open as real tabs in Chrome. Close the page when done; never close the context.
"""
import asyncio
import logging
from playwright.async_api import async_playwright, Browser, Playwright, Page

logger = logging.getLogger(__name__)

_browser: Browser | None = None
_playwright: Playwright | None = None
_lock = asyncio.Lock()

CDP_URL = "http://localhost:9222"


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


async def new_page() -> Page:
    """Return a new page in the user's real Chrome session.

    Opens as a visible tab. Caller MUST close the page when done:
        finally:
            await page.close()
    Never close the context — it's the user's live Chrome session.
    """
    browser = await get_browser()
    # Use the existing real Chrome context (has cookies, extensions, history)
    # Fall back to creating a context only if somehow none exist yet
    context = browser.contexts[0] if browser.contexts else await browser.new_context()
    return await context.new_page()
