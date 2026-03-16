"""Shared headless Chromium browser instance for all plugins."""
import asyncio
import logging
from playwright.async_api import async_playwright, Browser, Playwright

logger = logging.getLogger(__name__)

_browser: Browser | None = None
_playwright: Playwright | None = None
_lock = asyncio.Lock()

LAUNCH_ARGS = [
    "--disable-blink-features=AutomationControlled",
    "--no-sandbox",
    "--disable-dev-shm-usage",
    "--disable-gpu",
]


async def get_browser() -> Browser:
    """Return a shared Chromium instance, launching it if needed."""
    global _browser, _playwright
    async with _lock:
        if _browser is None or not _browser.is_connected():
            if _playwright is None:
                _playwright = await async_playwright().start()
            _browser = await _playwright.chromium.launch(
                headless=True,
                args=LAUNCH_ARGS,
            )
            logger.info("Launched shared headless Chromium")
    return _browser


async def new_context():
    """Return a new browser context with stealth user-agent."""
    browser = await get_browser()
    return await browser.new_context(
        user_agent=(
            "Mozilla/5.0 (X11; Linux x86_64) "
            "AppleWebKit/537.36 (KHTML, like Gecko) "
            "Chrome/124.0.0.0 Safari/537.36"
        ),
        viewport={"width": 1280, "height": 900},
        extra_http_headers={
            "Accept-Language": "en-US,en;q=0.9",
            "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        },
    )
