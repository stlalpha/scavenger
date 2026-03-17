import re
import logging
from datetime import datetime, timezone

from scavenger.dedup import content_hash
from scavenger.models import Listing, Profile
from scavenger.plugins.browser import new_page

logger = logging.getLogger(__name__)

SEARCH_URL = "https://www.ebay.com/sch/i.html"
PRICE_RE = re.compile(r"[\$£€]([0-9,]+(?:\.[0-9]{2})?)")

# Try these selectors in order — eBay occasionally restructures their DOM
ITEM_SELECTORS = [".s-item", "li.s-item", ".srp-results .s-item", "[data-viewport]"]

# eBay has two DOM generations: legacy (.s-item__*) and modern (.s-card__*)
TITLE_SELECTORS = [".s-card__title", ".s-item__title"]
LINK_SELECTORS = [".s-card__link", ".s-item__link"]
PRICE_SELECTORS = [".s-card__price", ".s-item__price"]
IMAGE_SELECTORS = [".s-card__image", ".s-item__image-img"]
LOCATION_SELECTORS = [".s-card__location", ".s-item__location"]


def _extract_price(text: str) -> float | None:
    m = PRICE_RE.search(text.replace(",", ""))
    return float(m.group(1)) if m else None


async def _query_first(parent, selectors: list[str]):
    """Try multiple CSS selectors, return the first matching element or None."""
    for sel in selectors:
        el = await parent.query_selector(sel)
        if el:
            return el
    return None


class EbayPlugin:
    plugin_id = "ebay"

    async def fetch(self, profile: Profile) -> list[Listing]:
        keywords = " ".join(
            kw if isinstance(kw, str) else kw[0] for kw in profile.keywords
        )
        try:
            return await self._scrape(keywords, profile)
        except Exception as e:
            logger.warning("eBay fetch failed: %s", e)
            return []

    async def _scrape(self, keywords: str, profile: Profile) -> list[Listing]:
        page = await new_page()
        try:
            params = (
                f"?_nkw={keywords.replace(' ', '+')}"
                "&_sop=10"    # sort: newly listed
                "&_ipg=50"    # 50 per page
            )
            if profile.price_min is not None:
                params += f"&_udlo={profile.price_min:.0f}"
            if profile.price_max is not None:
                params += f"&_udhi={profile.price_max:.0f}"

            await page.goto(SEARCH_URL + params, wait_until="domcontentloaded", timeout=30000)
            title = await page.title()
            logger.debug("eBay: page loaded, title=%r", title)

            if "Pardon Our Interruption" in title:
                logger.warning("eBay: bot detection page — try running Chrome with --remote-debugging-port=9222")
                return []

            # Try each selector until one matches
            item_selector = None
            for sel in ITEM_SELECTORS:
                try:
                    await page.wait_for_selector(sel, timeout=5000)
                    item_selector = sel
                    break
                except Exception:
                    continue

            if not item_selector:
                logger.warning("eBay: no listing elements found on page (title: %r)", title)
                # Log first 500 chars of page to help debug
                content = await page.content()
                logger.debug("eBay: page snippet: %s", content[:500])
                return []

            items = await page.query_selector_all(item_selector)
            listings = []
            now = datetime.now(timezone.utc)

            for item in items:
                try:
                    title_el = await _query_first(item, TITLE_SELECTORS)
                    link_el = await _query_first(item, LINK_SELECTORS)
                    price_el = await _query_first(item, PRICE_SELECTORS)
                    img_el = await _query_first(item, IMAGE_SELECTORS)

                    if not title_el or not link_el:
                        continue

                    item_title = (await title_el.inner_text()).strip()
                    # Strip eBay decoration text from titles
                    for noise in ("NEW LISTING", "Opens in a new window or tab"):
                        item_title = item_title.replace(noise, "").strip()
                    url = (await link_el.get_attribute("href") or "").split("?")[0]

                    if "Shop on eBay" in item_title or not url:
                        continue

                    price_text = await price_el.inner_text() if price_el else ""
                    price = _extract_price(price_text)

                    image_url = await img_el.get_attribute("src") if img_el else None
                    image_urls = [image_url] if image_url and not image_url.startswith("data:") else []

                    location_el = await _query_first(item, LOCATION_SELECTORS)
                    location = (await location_el.inner_text()).strip() if location_el else None

                    listings.append(Listing(
                        id=content_hash(url),
                        profile_id=profile.id,
                        source_id=self.plugin_id,
                        title=item_title,
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
            await page.close()

    async def supports_geo(self) -> bool:
        return False
