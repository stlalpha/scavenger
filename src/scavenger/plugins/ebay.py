import re
import logging
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from xml.etree import ElementTree as ET

import httpx

from scavenger.dedup import content_hash
from scavenger.models import Listing, Profile

logger = logging.getLogger(__name__)
RSS_URL = "https://rss.ebay.com/rss2/search"
PRICE_RE = re.compile(r"\$([0-9,]+(?:\.[0-9]{2})?)")


def _extract_price(text: str) -> float | None:
    m = PRICE_RE.search(text)
    return float(m.group(1).replace(",", "")) if m else None


class EbayPlugin:
    plugin_id = "ebay"

    async def fetch(self, profile: Profile) -> list[Listing]:
        keywords = " ".join(
            kw if isinstance(kw, str) else " ".join(kw) for kw in profile.keywords
        )
        try:
            async with httpx.AsyncClient(timeout=30.0) as client:
                resp = await client.get(RSS_URL, params={"kw": keywords, "country": "us", "siteid": "0"})
                resp.raise_for_status()
        except (httpx.HTTPError, httpx.TimeoutException) as e:
            logger.warning("eBay fetch failed: %s", e)
            return []
        return self._parse(resp.content, profile)

    def _parse(self, content: bytes, profile: Profile) -> list[Listing]:
        try:
            root = ET.fromstring(content)
        except ET.ParseError:
            return []
        channel = root.find("channel")
        if channel is None:
            return []
        now = datetime.now(timezone.utc)
        listings = []
        for item in channel.findall("item"):
            title_el, link_el = item.find("title"), item.find("link")
            if title_el is None or link_el is None:
                continue
            title = title_el.text or ""
            url = link_el.text or ""
            desc_el = item.find("description")
            description = desc_el.text or "" if desc_el is not None else ""
            pub_el = item.find("pubDate")
            try:
                pub_date = parsedate_to_datetime(pub_el.text) if pub_el is not None and pub_el.text else now
            except Exception:
                pub_date = now
            enclosure = item.find("enclosure")
            image_urls = [enclosure.get("url")] if enclosure is not None and enclosure.get("url") else []
            listings.append(Listing(
                id=content_hash(url),
                profile_id=profile.id,
                source_id=self.plugin_id,
                title=title,
                description=description,
                price=_extract_price(description),
                url=url,
                image_urls=image_urls,
                first_seen=pub_date,
                last_seen=now,
                relevance_score=0.0,
            ))
        return listings

    async def supports_geo(self) -> bool:
        return False
