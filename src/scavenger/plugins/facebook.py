import re
import logging
from datetime import datetime, timezone
from urllib.parse import quote_plus

from scavenger.dedup import content_hash
from scavenger.models import Listing, Profile
from scavenger.plugins.browser import new_page

logger = logging.getLogger(__name__)
PRICE_RE = re.compile(r"\$([0-9,]+(?:\.\d{2})?)")

BASE_URL = "https://www.facebook.com/marketplace"

# Facebook Marketplace DOM is deeply nested React output.
# These selectors target the search results grid items.
# FB changes class names frequently — we try multiple patterns.
ITEM_SELECTORS = [
    "div[class] > a[href*='/marketplace/item/']",
    "a[href*='/marketplace/item/']",
]


def _extract_price(text: str) -> float | None:
    m = PRICE_RE.search(text.replace(",", ""))
    return float(m.group(1)) if m else None


def _clean_fb_url(href: str) -> str:
    """Normalize a marketplace item URL to just the item path."""
    # Typical: /marketplace/item/123456789/?ref=...  -> strip params
    if "?" in href:
        href = href.split("?")[0]
    if not href.startswith("http"):
        href = f"https://www.facebook.com{href}"
    return href


class FacebookPlugin:
    plugin_id = "facebook"

    def __init__(self, location_id: str | None = None, radius_km: int = 80):
        self._location_id = location_id
        self._radius_km = radius_km

    async def fetch(self, profile: Profile) -> list[Listing]:
        keywords = " ".join(
            # Facebook has no OR syntax — use first variant per group
            kw if isinstance(kw, str) else kw[0] for kw in profile.keywords
        )
        try:
            return await self._scrape(keywords, profile)
        except Exception as e:
            logger.warning("Facebook fetch failed: %s", e)
            return []

    async def _scrape(self, keywords: str, profile: Profile) -> list[Listing]:
        page = await new_page()
        try:
            params = f"search/?query={quote_plus(keywords)}&sortBy=creation_time_descend&exact=false"
            if profile.price_min is not None:
                params += f"&minPrice={profile.price_min:.0f}"
            if profile.price_max is not None:
                params += f"&maxPrice={profile.price_max:.0f}"

            url = f"{BASE_URL}/{params}"
            await page.goto(url, wait_until="networkidle", timeout=45000)

            title = await page.title()
            if "log in" in title.lower() or "sign in" in title.lower():
                logger.warning("Facebook: not logged in — log into Facebook in the Chrome session first")
                return []

            # Scroll down a few times to load more results
            for _ in range(3):
                await page.evaluate("window.scrollBy(0, window.innerHeight)")
                await page.wait_for_timeout(1500)

            # Find item links
            items = []
            for sel in ITEM_SELECTORS:
                items = await page.query_selector_all(sel)
                if items:
                    break

            if not items:
                logger.warning("Facebook: no listings found (title: %r)", title)
                content = await page.content()
                logger.debug("Facebook: page snippet: %s", content[:500])
                return []

            listings = []
            now = datetime.now(timezone.utc)
            seen_urls = set()

            for item in items:
                try:
                    href = await item.get_attribute("href") or ""
                    if "/marketplace/item/" not in href:
                        continue

                    item_url = _clean_fb_url(href)
                    if item_url in seen_urls:
                        continue
                    seen_urls.add(item_url)

                    # The link element wraps the card — text children have title, price, location
                    text = (await item.inner_text()).strip()
                    lines = [l.strip() for l in text.split("\n") if l.strip()]

                    # FB card layout is typically: price, title, location, distance
                    # but order varies. Find price line, treat next non-price line as title.
                    price = None
                    item_title = ""
                    location = None

                    for line in lines:
                        if PRICE_RE.search(line) and price is None:
                            price = _extract_price(line)
                        elif not item_title and not PRICE_RE.search(line):
                            item_title = line

                    # Remaining lines after title are often location
                    if len(lines) > 2:
                        for line in lines[2:]:
                            if not PRICE_RE.search(line) and line != item_title:
                                location = line
                                break

                    if not item_title:
                        continue

                    # Image from the card
                    img_el = await item.query_selector("img")
                    image_url = await img_el.get_attribute("src") if img_el else None
                    image_urls = [image_url] if image_url and not image_url.startswith("data:") else []

                    listings.append(Listing(
                        id=content_hash(item_url),
                        profile_id=profile.id,
                        source_id=self.plugin_id,
                        title=item_title,
                        description="",
                        price=price,
                        url=item_url,
                        image_urls=image_urls,
                        location=location,
                        first_seen=now,
                        last_seen=now,
                        relevance_score=0.0,
                    ))
                except Exception as e:
                    logger.debug("Skipping Facebook item: %s", e)
                    continue

            logger.info("Facebook: found %d listings for '%s'", len(listings), keywords)
            return listings

        finally:
            await page.close()

    async def supports_geo(self) -> bool:
        return True
