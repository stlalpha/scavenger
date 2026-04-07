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


async def test_migrate_adds_ai_evaluation_column(db):
    await db.migrate()
    async with aiosqlite.connect(db.path) as conn:
        cursor = await conn.execute("PRAGMA table_info(listings)")
        columns = {row[1] for row in await cursor.fetchall()}
    assert "ai_evaluation" in columns


async def test_migrate_alters_existing_database_without_column(tmp_path):
    """Simulate upgrading a pre-M1.5 database that lacks the ai_evaluation column."""
    db_path = tmp_path / "legacy.db"
    # Create a database with the old schema (no ai_evaluation column)
    async with aiosqlite.connect(db_path) as conn:
        await conn.execute("""CREATE TABLE listings (
            id TEXT PRIMARY KEY, profile_id TEXT, source_id TEXT,
            title TEXT, description TEXT, price REAL, currency TEXT,
            condition TEXT, url TEXT, image_urls TEXT, location TEXT,
            first_seen TEXT, last_seen TEXT, relevance_score REAL, status TEXT
        )""")
        await conn.execute("INSERT INTO listings VALUES ('x','p','e','t','d',1.0,'USD',NULL,'u','[]',NULL,'2026-01-01','2026-01-01',50.0,'new')")
        await conn.commit()
    # Now open via Database and migrate
    database = Database(db_path)
    database._conn = await aiosqlite.connect(db_path)
    database._conn.row_factory = aiosqlite.Row
    await database.migrate()
    # Column must exist
    cursor = await database._conn.execute("PRAGMA table_info(listings)")
    columns = {row[1] for row in await cursor.fetchall()}
    assert "ai_evaluation" in columns
    # Existing row must be preserved
    cursor = await database._conn.execute("SELECT id FROM listings WHERE id='x'")
    row = await cursor.fetchone()
    assert row is not None
    await database.close()


async def test_migrate_is_idempotent(db):
    await db.migrate()
    await db.migrate()  # second call must not raise


async def test_update_listing_status(db):
    await db.upsert_listing(make_listing())
    await db.update_listing_status("abc123", "saved")
    fetched = await db.get_listing("abc123")
    assert fetched.status == "saved"


async def test_get_active_listings_excludes_dismissed(db):
    await db.upsert_listing(make_listing(id="a1", url="https://ebay.com/1"))
    await db.upsert_listing(make_listing(id="a2", url="https://ebay.com/2"))
    await db.update_listing_status("a1", "dismissed")
    active = await db.get_active_listings(limit=10)
    ids = [l.id for l in active]
    assert "a1" not in ids
    assert "a2" in ids


async def test_busy_timeout_is_set(db):
    """Verify busy_timeout is configured on connection."""
    cursor = await db._conn.execute("PRAGMA busy_timeout")
    row = await cursor.fetchone()
    assert row[0] == 5000


async def test_update_listing_status_rejects_invalid(db):
    await db.upsert_listing(make_listing())
    with pytest.raises(ValueError, match="Invalid status"):
        await db.update_listing_status("abc123", "bogus")


async def test_get_existing_ids_large_batch(db):
    """Verify get_existing_ids handles >999 IDs without error."""
    for i in range(5):
        await db.upsert_listing(make_listing(id=f"known-{i}", url=f"https://ebay.com/{i}"))
    all_ids = [f"known-{i}" for i in range(5)] + [f"unknown-{i}" for i in range(1495)]
    result = await db.get_existing_ids(all_ids)
    assert result == {f"known-{i}" for i in range(5)}


async def test_ai_evaluation_persists_through_upsert(db):
    from datetime import datetime, timezone
    await db.migrate()
    now = datetime.now(timezone.utc)
    listing = Listing(
        id="ai1", profile_id="p1", source_id="ebay",
        title="Sony 85mm", url="https://ebay.com/ai1",
        first_seen=now, last_seen=now, relevance_score=80.0,
        ai_evaluation='{"relevant": true, "reason": "Great", "notable": null, "escalate": false}',
    )
    await db.upsert_listing(listing)
    fetched = await db.get_listing("ai1")
    assert fetched.ai_evaluation is not None
    assert "Great" in fetched.ai_evaluation
