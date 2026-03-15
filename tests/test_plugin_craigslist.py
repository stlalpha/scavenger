import pytest
import respx
import httpx
from pathlib import Path
from scavenger.plugins.craigslist import CraigslistPlugin
from scavenger.models import Profile

FIXTURES = Path(__file__).parent / "fixtures"


@pytest.fixture
def profile():
    return Profile(
        id="sony", name="Sony Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken"],
        sources=["craigslist"],
    )


@pytest.fixture
def plugin():
    return CraigslistPlugin(cities=["sfbay"])


@respx.mock
async def test_fetch_returns_listings(plugin, profile):
    respx.get("https://sfbay.craigslist.org/search/sss").mock(
        return_value=httpx.Response(200, content=(FIXTURES / "craigslist_rss.xml").read_bytes())
    )
    listings = await plugin.fetch(profile)
    assert len(listings) == 2


@respx.mock
async def test_price_extracted_from_title(plugin, profile):
    respx.get("https://sfbay.craigslist.org/search/sss").mock(
        return_value=httpx.Response(200, content=(FIXTURES / "craigslist_rss.xml").read_bytes())
    )
    listings = await plugin.fetch(profile)
    assert listings[0].price == 650.0


@respx.mock
async def test_image_extracted(plugin, profile):
    respx.get("https://sfbay.craigslist.org/search/sss").mock(
        return_value=httpx.Response(200, content=(FIXTURES / "craigslist_rss.xml").read_bytes())
    )
    listings = await plugin.fetch(profile)
    assert len(listings[0].image_urls) == 1


@respx.mock
async def test_http_error_returns_empty(plugin, profile):
    respx.get("https://sfbay.craigslist.org/search/sss").mock(return_value=httpx.Response(500))
    assert await plugin.fetch(profile) == []


async def test_plugin_id():
    assert CraigslistPlugin.plugin_id == "craigslist"

async def test_supports_geo():
    assert await CraigslistPlugin(cities=["sfbay"]).supports_geo() is True
