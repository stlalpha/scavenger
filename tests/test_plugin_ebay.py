import pytest
import respx
import httpx
from pathlib import Path
from scavenger.plugins.ebay import EbayPlugin
from scavenger.models import Profile
from scavenger.dedup import content_hash

FIXTURES = Path(__file__).parent / "fixtures"
RSS_URL = "https://rss.ebay.com/rss2/search"


@pytest.fixture
def profile():
    return Profile(
        id="sony", name="Sony Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken"],
        sources=["ebay"], price_min=50.0, price_max=800.0,
    )


@respx.mock
async def test_fetch_returns_listings(profile):
    respx.get(RSS_URL).mock(return_value=httpx.Response(200, content=(FIXTURES / "ebay_rss.xml").read_bytes()))
    listings = await EbayPlugin().fetch(profile)
    assert len(listings) == 2


@respx.mock
async def test_listing_fields(profile):
    respx.get(RSS_URL).mock(return_value=httpx.Response(200, content=(FIXTURES / "ebay_rss.xml").read_bytes()))
    listings = await EbayPlugin().fetch(profile)
    first = listings[0]
    assert "Sony 85mm" in first.title
    assert first.source_id == "ebay"
    assert first.url == "https://www.ebay.com/itm/123456789012"
    assert first.price == 249.99
    assert len(first.image_urls) == 1


@respx.mock
async def test_listing_id_is_content_hash(profile):
    respx.get(RSS_URL).mock(return_value=httpx.Response(200, content=(FIXTURES / "ebay_rss.xml").read_bytes()))
    listings = await EbayPlugin().fetch(profile)
    assert listings[0].id == content_hash("https://www.ebay.com/itm/123456789012")


@respx.mock
async def test_http_error_returns_empty(profile):
    respx.get(RSS_URL).mock(return_value=httpx.Response(503))
    assert await EbayPlugin().fetch(profile) == []


async def test_plugin_id():
    assert EbayPlugin.plugin_id == "ebay"

async def test_supports_geo():
    assert await EbayPlugin().supports_geo() is False
