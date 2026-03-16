import re
import logging
from datetime import datetime, timezone
from playwright.async_api import async_playwright, Browser, BrowserContext

from scavenger.dedup import content_hash
from scavenger.models import Listing, Profile

logger = logging.getLogger(__name__)

SEARCH_URL = "https://www.ebay.com/sch/i.html"
PRICE_RE = re.compile(r"[\$£€]([0-9,]+(?:\.[0-9]{2})?)")

# Reuse browser across polls — launched lazily, replaced if disconnected
_browser: Browser | None = None
_playwright_instance = None


async def _get_browser() -> Browser:
    global _browser, _playwright_instance
    if _browser is None or not _browser.is_connected():
        if _playwright_instance is None:
            _playwright_instance = await async_playwright().start()
        _browser = await _playwright_instance.chromium.launch(headless=True)
        logger.info("Launched headless Chromium for eBay plugin")
    return _browser


def _extract_price(text: str) -> float | None:
    m = PRICE_RE.search(text.replace(",", ""))
    return float(m.group(1)) if m else None


class EbayPlugin:
    plugin_id = "ebay"

    async def fetch(self, profile: Profile) -> list[Listing]:
        keywords = " ".join(
            kw if isinstance(kw, str) else " ".join(kw) for kw in profile.keywords
        )
        try:
            return await self._scrape(keywords, profile)
        except Exception as e:
            logger.warning("eBay fetch failed: %s", e)
            return []

    async def _scrape(self, keywords: str, profile: Profile) -> list[Listing]:
        browser = await _get_browser()
        context: BrowserContext = await browser.new_context(
            user_agent=(
                "Mozilla/5.0 (X11; Linux x86_64) "
                "AppleWebKit/537.36 (KHTML, like Gecko) "
                "Chrome/124.0.0.0 Safari/537.36"
            ),
            viewport={"width": 1280, "height": 900},
        )
        page = await context.new_page()
        try:
            params = (
                f"?_nkw={keywords.replace(' ', '+')}"
                "&_sop=10"    # sort: newly listed
                "&_ipg=50"    # 50 results per page
            )
            if profile.price_min is not None:
                params += f"&_udlo={profile.price_min:.0f}"
            if profile.price_max is not None:
                params += f"&_udhi={profile.price_max:.0f}"

            await page.goto(SEARCH_URL + params, wait_until="domcontentloaded", timeout=30000)
            await page.wait_for_selector(".s-item", timeout=10000)

            items = await page.query_selector_all(".s-item")
            listings = []
            now = datetime.now(timezone.utc)

            for item in items:
                try:
                    title_el = await item.query_selector(".s-item__title")
                    link_el = await item.query_selector(".s-item__link")
                    price_el = await item.query_selector(".s-item__price")
                    img_el = await item.query_selector(".s-item__image-img")

                    if not title_el or not link_el:
                        continue

                    title = (await title_el.inner_text()).strip()
                    url = (await link_el.get_attribute("href") or "").split("?")[0]

                    # Skip the "Shop on eBay" placeholder card
                    if "Shop on eBay" in title or not url:
                        continue

                    price_text = await price_el.inner_text() if price_el else ""
                    price = _extract_price(price_text)

                    image_url = await img_el.get_attribute("src") if img_el else None
                    image_urls = [image_url] if image_url and not image_url.startswith("data:") else []

                    location_el = await item.query_selector(".s-item__location")
                    location = (await location_el.inner_text()).strip() if location_el else None

                    listings.append(Listing(
                        id=content_hash(url),
                        profile_id=profile.id,
                        source_id=self.plugin_id,
                        title=title,
                        description="",
                        price=price,
                        url=url,
                        image_urls=image_urls,
                        location=location,
                        first_seen=now,
                        last_seen=now,
                        relevance_score=0.0,
                    ))
                except Exception as e:
                    logger.debug("Skipping eBay item: %s", e)
                    continue

            logger.info("eBay: found %d listings for '%s'", len(listings), keywords)
            return listings

        finally:
            await context.close()

    async def supports_geo(self) -> bool:
        return False
