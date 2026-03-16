import re
import logging
from datetime import datetime, timezone

from scavenger.dedup import content_hash
from scavenger.models import Listing, Profile
from scavenger.plugins.browser import new_context

logger = logging.getLogger(__name__)

SEARCH_URL = "https://www.ebay.com/sch/i.html"
PRICE_RE = re.compile(r"[\$£€]([0-9,]+(?:\.[0-9]{2})?)")


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
        context = await new_context()
        page = await context.new_page()

        # Mask webdriver flag
        await page.add_init_script(
            "Object.defineProperty(navigator, 'webdriver', {get: () => undefined})"
        )

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

            logger.debug("eBay: navigating to %s", SEARCH_URL + params)
            await page.goto(SEARCH_URL + params, wait_until="domcontentloaded", timeout=30000)
            logger.debug("eBay: page loaded, title=%r", await page.title())

            # Wait for listings — eBay may show a verification page if blocked
            try:
                await page.wait_for_selector(".s-item", timeout=15000)
            except Exception:
                title = await page.title()
                logger.warning("eBay: no listings found (page title: %r) — possible bot block", title)
                return []

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
