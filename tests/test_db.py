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


async def test_get_listing_returns_none_for_missing_id(db):
    result = await db.get_listing("nonexistent")
    assert result is None

async def test_get_listings_filter_by_profile_id(db):
    await db.upsert_listing(make_listing(id="a1", url="https://ebay.com/1", profile_id="p1"))
    await db.upsert_listing(make_listing(id="a2", url="https://ebay.com/2", profile_id="p2"))
    listings = await db.get_listings(profile_id="p1", limit=10)
    assert len(listings) == 1
    assert listings[0].profile_id == "p1"

async def test_upsert_same_price_does_not_add_history(db):
    await db.upsert_listing(make_listing(price=100.0))
    await db.upsert_listing(make_listing(price=100.0))
    history = await db.get_price_history("abc123")
    assert len(history) == 1  # only one entry, not two

async def test_upsert_no_price_does_not_add_history(db):
    await db.upsert_listing(make_listing(price=None))
    history = await db.get_price_history("abc123")
    assert len(history) == 0

async def test_update_source_state_insert(db):
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc)
    await db.update_source_state("ebay", last_polled=now, consecutive_errors=0)
    async with aiosqlite.connect(db.path) as conn:
        conn.row_factory = aiosqlite.Row
        cursor = await conn.execute("SELECT * FROM sources WHERE plugin_id=?", ("ebay",))
        row = await cursor.fetchone()
    assert row is not None
    assert dict(row)["consecutive_errors"] == 0

async def test_update_source_state_upsert(db):
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc)
    await db.update_source_state("ebay", last_polled=now, consecutive_errors=0)
    await db.update_source_state("ebay", last_polled=now, consecutive_errors=3)
    async with aiosqlite.connect(db.path) as conn:
        conn.row_factory = aiosqlite.Row
        cursor = await conn.execute("SELECT consecutive_errors FROM sources WHERE plugin_id=?", ("ebay",))
        row = await cursor.fetchone()
    assert dict(row)["consecutive_errors"] == 3

async def test_image_urls_round_trip(db):
    urls = ["https://img1.jpg", "https://img2.jpg"]
    await db.upsert_listing(make_listing(image_urls=urls))
    fetched = await db.get_listing("abc123")
    assert fetched.image_urls == urls
