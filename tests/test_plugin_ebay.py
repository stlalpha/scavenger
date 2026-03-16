from datetime import datetime, timezone
from unittest.mock import AsyncMock, patch
from scavenger.plugins.ebay import EbayPlugin
from scavenger.models import Profile, Listing
from scavenger.dedup import content_hash


def make_profile() -> Profile:
    return Profile(
        id="sony", name="Sony Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken"],
        sources=["ebay"], price_min=50.0, price_max=800.0,
    )


def make_listing(id: str = "abc", url: str = "https://www.ebay.com/itm/123456789012") -> Listing:
    now = datetime.now(timezone.utc)
    return Listing(
        id=content_hash(url), profile_id="sony", source_id="ebay",
        title="Sony 85mm f/1.4 A-mount Lens",
        url=url, price=249.99, image_urls=["https://i.ebayimg.com/img1.jpg"],
        first_seen=now, last_seen=now, relevance_score=0.0,
    )


async def test_fetch_calls_scrape(monkeypatch):
    listings = [make_listing()]
    plugin = EbayPlugin()
    monkeypatch.setattr(plugin, "_scrape", AsyncMock(return_value=listings))
    result = await plugin.fetch(make_profile())
    assert result == listings


async def test_fetch_returns_empty_on_scrape_error(monkeypatch):
    plugin = EbayPlugin()
    monkeypatch.setattr(plugin, "_scrape", AsyncMock(side_effect=Exception("network error")))
    result = await plugin.fetch(make_profile())
    assert result == []


async def test_fetch_returns_multiple_listings(monkeypatch):
    listings = [make_listing(url=f"https://www.ebay.com/itm/{i}") for i in range(5)]
    plugin = EbayPlugin()
    monkeypatch.setattr(plugin, "_scrape", AsyncMock(return_value=listings))
    result = await plugin.fetch(make_profile())
    assert len(result) == 5


async def test_listing_id_is_content_hash(monkeypatch):
    url = "https://www.ebay.com/itm/123456789012"
    listing = make_listing(url=url)
    plugin = EbayPlugin()
    monkeypatch.setattr(plugin, "_scrape", AsyncMock(return_value=[listing]))
    result = await plugin.fetch(make_profile())
    assert result[0].id == content_hash(url)


async def test_plugin_id():
    assert EbayPlugin.plugin_id == "ebay"


async def test_supports_geo():
    assert await EbayPlugin().supports_geo() is False
