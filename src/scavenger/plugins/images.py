"""Lightweight image URL extraction from listing detail pages.

No browser needed — just httpx GET + regex per source.
"""
import re
import logging
import httpx

logger = logging.getLogger(__name__)

# eBay embeds all gallery images as JSON in the page
_EBAY_IMG_RE = re.compile(r'"https://i\.ebayimg\.com/images/g/[^"]+/s-l\d+\.\w+"')
# Craigslist puts image IDs in a JS array
_CL_IMG_RE = re.compile(r'https://images\.craigslist\.org/[a-zA-Z0-9_]+_\d+x\d+\.jpg')
# Facebook embeds scontent URLs
_FB_IMG_RE = re.compile(r'"(https://scontent[^"]+)"')

_HEADERS = {"User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"}
_client: httpx.AsyncClient | None = None


def _get_client() -> httpx.AsyncClient:
    global _client
    if _client is None or _client.is_closed:
        _client = httpx.AsyncClient(timeout=15.0, follow_redirects=True, headers=_HEADERS)
    return _client


async def fetch_listing_images(url: str, source_id: str) -> list[str]:
    """Fetch a listing page and extract all product image URLs."""
    if not url or not url.startswith("http"):
        return []
    try:
        resp = await _get_client().get(url)
        if resp.status_code != 200:
            return []
        html = resp.text
    except Exception as e:
        logger.debug("Image fetch failed for %s: %s", url[:60], e)
        return []

    if source_id == "ebay":
        return _extract_ebay(html)
    elif source_id == "craigslist":
        return _extract_craigslist(html)
    elif source_id == "facebook":
        return _extract_facebook(html)
    return []


def _extract_ebay(html: str) -> list[str]:
    """Extract eBay gallery images — prefer s-l500 size."""
    raw = _EBAY_IMG_RE.findall(html)
    seen: set[str] = set()
    images: list[str] = []
    for url in raw:
        url = url.strip('"')
        # Normalize to s-l500
        normalized = re.sub(r'/s-l\d+\.', '/s-l500.', url)
        if normalized not in seen:
            seen.add(normalized)
            images.append(normalized)
    return images


def _extract_craigslist(html: str) -> list[str]:
    """Extract Craigslist gallery images."""
    raw = _CL_IMG_RE.findall(html)
    seen: set[str] = set()
    images: list[str] = []
    for url in raw:
        # Normalize to 600x450
        normalized = re.sub(r'_\d+x\d+\.jpg', '_600x450.jpg', url)
        if normalized not in seen:
            seen.add(normalized)
            images.append(normalized)
    return images


def _extract_facebook(html: str) -> list[str]:
    """Extract Facebook marketplace images."""
    raw = _FB_IMG_RE.findall(html)
    seen: set[str] = set()
    images: list[str] = []
    for url in raw:
        # Filter out tiny avatars/icons
        if "emoji" in url or "p50x50" in url or "p36x36" in url:
            continue
        if url not in seen:
            seen.add(url)
            images.append(url)
    return images[:10]  # cap at 10
