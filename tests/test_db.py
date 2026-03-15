# tests/test_db.py
import pytest
import aiosqlite
from datetime import datetime, timezone
from scavenger.db import Database
from scavenger.models import Listing


@pytest.fixture
async def db(tmp_path):
    database = Database(tmp_path / "test.db")
    await database.init()
    yield database
    await database.close()


def make_listing(**overrides) -> Listing:
    now = datetime.now(timezone.utc)
    defaults = dict(
        id="abc123",
        profile_id="p1",
        source_id="ebay",
        title="Sony 85mm f/1.4",
        url="https://ebay.com/itm/123",
        image_urls=["https://i.ebayimg.com/img1.jpg"],
        first_seen=now,
        last_seen=now,
        relevance_score=80.0,
        price=199.99,
    )
    defaults.update(overrides)
    return Listing(**defaults)


async def test_init_creates_tables(db):
    async with aiosqlite.connect(db.path) as conn:
        cursor = await conn.execute("SELECT name FROM sqlite_master WHERE type='table'")
        tables = {row[0] for row in await cursor.fetchall()}
    assert {"listings", "price_history", "image_cache", "sources"}.issubset(tables)


async def test_upsert_new_listing(db):
    is_new = await db.upsert_listing(make_listing())
    assert is_new is True


async def test_upsert_existing_returns_false(db):
    await db.upsert_listing(make_listing())
    is_new = await db.upsert_listing(make_listing())
    assert is_new is False


async def test_get_listing_by_id(db):
    await db.upsert_listing(make_listing())
    fetched = await db.get_listing("abc123")
    assert fetched is not None
    assert fetched.title == "Sony 85mm f/1.4"


async def test_get_listings_by_status(db):
    await db.upsert_listing(make_listing(id="a1", url="https://ebay.com/1"))
    await db.upsert_listing(make_listing(id="a2", url="https://ebay.com/2"))
    listings = await db.get_listings(status="new", limit=10)
    assert len(listings) == 2


async def test_price_history_recorded_on_change(db):
    await db.upsert_listing(make_listing(price=100.0))
    await db.upsert_listing(make_listing(price=80.0))
    history = await db.get_price_history("abc123")
    prices = [row["price"] for row in history]
    assert 100.0 in prices
    assert 80.0 in prices
