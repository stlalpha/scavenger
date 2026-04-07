import re
import asyncio
import logging
from datetime import datetime, timezone
from urllib.parse import quote_plus

from scavenger.dedup import content_hash
from scavenger.models import Listing, Profile
from scavenger.plugins.browser import new_page
from scavenger.plugins.craigslist_cities import cities_for_zip, NATIONAL_METROS

logger = logging.getLogger(__name__)
PRICE_RE = re.compile(r"\$([0-9,]+(?:\.[0-9]{2})?)")
MAX_CONCURRENT_CITIES = 3


def _extract_price(text: str) -> float | None:
    m = PRICE_RE.search(text)
    return float(m.group(1).replace(",", "")) if m else None


_BG_URL_RE = re.compile(r'url\(["\']?(https?://[^"\')\s]+)')


async def _extract_cl_image(item) -> list[str]:
    """Extract image URL from a Craigslist search result item."""
    # 1. Direct img tag with src or data-src
    for sel in ("img", ".swipe img", ".gallery img"):
        img = await item.query_selector(sel)
        if img:
            for attr in ("src", "data-src"):
                url = await img.get_attribute(attr)
                if url and url.startswith("http") and "data:" not in url:
                    return [url]
    # 2. Gallery div with data-ids (Craigslist image hash format)
    gallery = await item.query_selector("[data-ids]")
    if gallery:
        ids_str = await gallery.get_attribute("data-ids") or ""
        if ids_str:
            first_id = ids_str.split(",")[0].split(":")[-1].strip()
            if first_id:
                return [f"https://images.craigslist.org/{first_id}_300x300.jpg"]
    # 3. Background image in style attribute
    for sel in (".swipe", ".gallery", "[style*=background]"):
        el = await item.query_selector(sel)
        if el:
            style = await el.get_attribute("style") or ""
            m = _BG_URL_RE.search(style)
            if m:
                return [m.group(1)]
    return []


class CraigslistPlugin:
    plugin_id = "craigslist"

    def __init__(self, cities: list[str] | None = None, home_zip: str | None = None):
        self._explicit_cities = cities
        self._home_zip = home_zip
        self._resolved_cities: list[str] | None = None
        self._city_lock = asyncio.Lock()

    async def _get_cities(self) -> list[str]:
        if self._explicit_cities:
            return self._explicit_cities
        async with self._city_lock:
            if self._resolved_cities is None:
                self._resolved_cities = await cities_for_zip(self._home_zip) if self._home_zip else NATIONAL_METROS
        return self._resolved_cities

    async def fetch(self, profile: Profile) -> list[Listing]:
        # Craigslist doesn't support grouped OR — use first variant per group
        keywords = " ".join(
            kw if isinstance(kw, str) else kw[0]
            for kw in profile.keywords
        )
        cities = await self._get_cities()
        results = await asyncio.gather(
            *[self._fetch_city(city, keywords, profile) for city in cities],
            return_exceptions=False,
        )
        return [listing for city_listings in results for listing in city_listings]

    async def _fetch_city(self, city: str, keywords: str, profile: Profile) -> list[Listing]:
        page = await new_page()
        try:
            url = f"https://{city}.craigslist.org/search/sss?query={quote_plus(keywords)}&sort=date&hasPic=1"
            await page.goto(url, wait_until="domcontentloaded", timeout=30000)

            # Craigslist has two result formats depending on the city/view
            item_selector = None
            for sel in [".cl-search-result", ".result-row", "li.result-row"]:
                try:
                    await page.wait_for_selector(sel, timeout=8000)
                    item_selector = sel
                    break
                except Exception:
                    continue

            if not item_selector:
                logger.warning("Craigslist %s: no results found", city)
                return []

            items = await page.query_selector_all(item_selector)
            listings = []
            now = datetime.now(timezone.utc)

            for item in items:
                try:
                    # Try both old and new Craigslist DOM structures
                    title_el = (
                        await item.query_selector(".posting-title .label") or
                        await item.query_selector(".result-title") or
                        await item.query_selector("a.titlestring")
                    )
                    link_el = (
                        await item.query_selector("a.posting-title") or
                        await item.query_selector("a.result-title") or
                        await item.query_selector("a.titlestring")
                    )

                    if not title_el or not link_el:
                        continue

                    title = (await title_el.inner_text()).strip()
                    item_url = await link_el.get_attribute("href") or ""
                    if not item_url.startswith("http"):
                        item_url = f"https://{city}.craigslist.org{item_url}"

                    price_el = (
                        await item.query_selector(".priceinfo") or
                        await item.query_selector(".result-price")
                    )
                    price_text = await price_el.inner_text() if price_el else ""
                    price = _extract_price(price_text)

                    image_urls = await _extract_cl_image(item)

                    listings.append(Listing(
                        id=content_hash(item_url),
                        profile_id=profile.id,
                        source_id=self.plugin_id,
                        title=title,
                        description="",
                        price=price,
                        location=city,
                        url=item_url,
                        image_urls=image_urls,
                        first_seen=now,
                        last_seen=now,
                        relevance_score=0.0,
                    ))
                except Exception as e:
                    logger.debug("Skipping Craigslist item in %s: %s", city, e)
                    continue

            logger.info("Craigslist %s: found %d listings for '%s'", city, len(listings), keywords)
            return listings

        except Exception as e:
            logger.warning("Craigslist fetch failed for %s: %s", city, e)
            return []
        finally:
            await page.close()

    async def supports_geo(self) -> bool:
        return True
