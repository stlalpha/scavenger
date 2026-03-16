import pytest
from datetime import datetime, timezone
from pathlib import Path
from scavenger.tui.data import DataLayer
from scavenger.db import Database
from scavenger.models import Listing


async def make_db_with_listings(tmp_path: Path) -> Database:
    db = Database(tmp_path / "test.db")
    await db.init()
    now = datetime.now(timezone.utc)
    for i in range(3):
        await db.upsert_listing(Listing(
            id=f"id{i}", profile_id="p1", source_id="ebay",
            title=f"Listing {i}", url=f"https://ebay.com/{i}",
            first_seen=now, last_seen=now, relevance_score=80.0,
            price=float(100 + i * 50),
        ))
    return db


async def test_get_listings_returns_all(tmp_path):
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    listings = await layer.get_listings(profile_id=None, limit=10)
    assert len(listings) == 3
    await db.close()


async def test_get_listings_filters_by_profile(tmp_path):
    db = await make_db_with_listings(tmp_path)
    now = datetime.now(timezone.utc)
    await db.upsert_listing(Listing(
        id="other", profile_id="p2", source_id="ebay",
        title="Other", url="https://ebay.com/other",
        first_seen=now, last_seen=now, relevance_score=60.0,
    ))
    layer = DataLayer(db)
    listings = await layer.get_listings(profile_id="p1", limit=10)
    assert all(l.profile_id == "p1" for l in listings)
    assert len(listings) == 3
    await db.close()


async def test_get_profile_stats_counts_new(tmp_path):
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    stats = await layer.get_profile_stats()
    assert stats.get("p1", 0) == 3
    await db.close()


async def test_mark_status_updates_db(tmp_path):
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    await layer.mark_status("id0", "dismissed")
    listing = await db.get_listing("id0")
    assert listing.status == "dismissed"
    await db.close()


async def test_get_listings_excludes_dismissed(tmp_path):
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    await layer.mark_status("id0", "dismissed")
    listings = await layer.get_listings(profile_id=None, limit=10)
    ids = [l.id for l in listings]
    assert "id0" not in ids
    await db.close()
