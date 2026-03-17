"""Scrape all images from a listing's detail page."""
import logging
from scavenger.plugins.browser import new_page

logger = logging.getLogger(__name__)


async def scrape_detail_images(url: str) -> list[str]:
    """Visit a listing URL and return all product image URLs found."""
    if not url or not url.startswith("http"):
        return []
    page = await new_page()
    try:
        await page.goto(url, wait_until="domcontentloaded", timeout=20000)
        await page.wait_for_timeout(2000)

        images: list[str] = []
        seen: set[str] = set()

        # Strategy 1: eBay — look for gallery images
        for sel in [
            "img[src*='ebayimg.com']",
            "[data-testid='ux-image-carousel'] img",
            ".ux-image-carousel img",
        ]:
            for img in await page.query_selector_all(sel):
                for attr in ("src", "data-src", "data-zoom-src"):
                    src = await img.get_attribute(attr)
                    if src and src.startswith("http") and "data:" not in src and src not in seen:
                        # Prefer larger versions
                        src = src.replace("/s-l64.", "/s-l500.").replace("/s-l140.", "/s-l500.").replace("/s-l225.", "/s-l500.")
                        if src not in seen:
                            seen.add(src)
                            images.append(src)

        # Strategy 2: Craigslist — gallery images
        for sel in [
            ".gallery img",
            ".swipe img",
            "#thumbs a img",
            ".slide img",
        ]:
            for img in await page.query_selector_all(sel):
                src = await img.get_attribute("src")
                if src and src.startswith("http") and src not in seen:
                    seen.add(src)
                    images.append(src)

        # Strategy 3: Facebook — carousel images
        for sel in [
            "img[src*='scontent']",
            "[data-visualcompletion] img",
        ]:
            for img in await page.query_selector_all(sel):
                src = await img.get_attribute("src")
                if src and src.startswith("http") and "emoji" not in src and src not in seen:
                    seen.add(src)
                    images.append(src)

        # Strategy 4: Generic fallback — any large image
        if not images:
            for img in await page.query_selector_all("img"):
                src = await img.get_attribute("src")
                if not src or not src.startswith("http") or "data:" in src:
                    continue
                # Try to filter small icons/buttons
                w = await img.get_attribute("width")
                h = await img.get_attribute("height")
                if w and h:
                    try:
                        if int(w) < 100 or int(h) < 100:
                            continue
                    except ValueError:
                        pass
                if src not in seen:
                    seen.add(src)
                    images.append(src)

        logger.debug("Detail scrape %s: found %d images", url[:60], len(images))
        return images

    except Exception as e:
        logger.debug("Detail scrape failed for %s: %s", url[:60], e)
        return []
    finally:
        await page.close()
