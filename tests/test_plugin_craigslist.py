from unittest.mock import AsyncMock, patch
from datetime import datetime, timezone
from scavenger.plugins.craigslist import CraigslistPlugin
from scavenger.models import Profile, Listing
from scavenger.dedup import content_hash


def make_profile() -> Profile:
    return Profile(
        id="sony", name="Sony Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken"],
        sources=["craigslist"],
    )


def make_listing(city: str = "sfbay") -> Listing:
    now = datetime.now(timezone.utc)
    url = f"https://{city}.craigslist.org/ele/d/test/1234567890.html"
    return Listing(
        id=content_hash(url), profile_id="sony", source_id="craigslist",
        title="Sony A-mount 70-200mm f/2.8 G SSM - $650",
        url=url, price=650.0, location=city,
        image_urls=["https://images.craigslist.org/test.jpg"],
        first_seen=now, last_seen=now, relevance_score=0.0,
    )


async def test_fetch_calls_fetch_city(monkeypatch):
    listings = [make_listing()]
    plugin = CraigslistPlugin(cities=["sfbay"])
    monkeypatch.setattr(plugin, "_fetch_city", AsyncMock(return_value=listings))
    result = await plugin.fetch(make_profile())
    assert result == listings


async def test_fetch_aggregates_multiple_cities(monkeypatch):
    plugin = CraigslistPlugin(cities=["sfbay", "newyork"])
    monkeypatch.setattr(plugin, "_fetch_city", AsyncMock(return_value=[make_listing()]))
    result = await plugin.fetch(make_profile())
    assert len(result) == 2


async def test_fetch_returns_empty_when_all_cities_fail(monkeypatch):
    plugin = CraigslistPlugin(cities=["sfbay"])
    # _fetch_city has its own error handling and returns [] on failure
    monkeypatch.setattr(plugin, "_fetch_city", AsyncMock(return_value=[]))
    result = await plugin.fetch(make_profile())
    assert result == []


async def test_plugin_id():
    assert CraigslistPlugin.plugin_id == "craigslist"


async def test_supports_geo():
    assert await CraigslistPlugin(cities=["sfbay"]).supports_geo() is True
