import re
import asyncio
import logging
from datetime import datetime, timezone
from xml.etree import ElementTree as ET

from scavenger.dedup import content_hash
from scavenger.models import Listing, Profile
from scavenger.plugins.browser import new_context

logger = logging.getLogger(__name__)

DEFAULT_CITIES = ["sfbay", "newyork", "losangeles", "chicago", "seattle"]
PRICE_RE = re.compile(r"\$([0-9,]+(?:\.[0-9]{2})?)")


def _extract_price(text: str) -> float | None:
    m = PRICE_RE.search(text)
    return float(m.group(1).replace(",", "")) if m else None


class CraigslistPlugin:
    plugin_id = "craigslist"

    def __init__(self, cities: list[str] | None = None):
        self._cities = cities or DEFAULT_CITIES

    async def fetch(self, profile: Profile) -> list[Listing]:
        keywords = " ".join(
            kw if isinstance(kw, str) else " ".join(kw) for kw in profile.keywords
        )
        results = await asyncio.gather(
            *[self._fetch_city(city, keywords, profile) for city in self._cities],
            return_exceptions=False,
        )
        return [listing for city_listings in results for listing in city_listings]

    async def _fetch_city(self, city: str, keywords: str, profile: Profile) -> list[Listing]:
        url = f"https://{city}.craigslist.org/search/sss"
        context = await new_context()
        page = await context.new_page()
        try:
            await page.goto(
                f"{url}?query={keywords.replace(' ', '+')}&sort=date",
                wait_until="domcontentloaded",
                timeout=30000,
            )
            try:
                await page.wait_for_selector(".cl-search-result", timeout=10000)
            except Exception:
                logger.warning("Craigslist %s: no results found", city)
                return []

            items = await page.query_selector_all(".cl-search-result")
            listings = []
            now = datetime.now(timezone.utc)

            for item in items:
                try:
                    title_el = await item.query_selector(".posting-title .label")
                    link_el = await item.query_selector("a.posting-title")
                    price_el = await item.query_selector(".priceinfo")
                    img_el = await item.query_selector("img")

                    if not title_el or not link_el:
                        continue

                    title = (await title_el.inner_text()).strip()
                    item_url = await link_el.get_attribute("href") or ""
                    if not item_url.startswith("http"):
                        item_url = f"https://{city}.craigslist.org{item_url}"

                    price_text = await price_el.inner_text() if price_el else ""
                    price = _extract_price(price_text)

                    image_url = await img_el.get_attribute("src") if img_el else None
                    image_urls = [image_url] if image_url and not image_url.startswith("data:") else []

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
            await context.close()

    async def supports_geo(self) -> bool:
        return True
