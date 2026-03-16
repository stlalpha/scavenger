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


async def test_mark_snoozed_does_not_crash_get_listings(tmp_path):
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    await layer.mark_status("id0", "snoozed")
    # Must not raise ValidationError
    listings = await layer.get_listings(profile_id=None, limit=10)
    assert all(l.id != "id0" or l.status == "snoozed" for l in listings)
    await db.close()


async def test_mark_seen_transitions_new_to_seen(tmp_path):
    """mark_seen should change status from 'new' to 'seen'."""
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    listing = await db.get_listing("id0")
    assert listing.status == "new"
    await layer.mark_seen("id0")
    listing = await db.get_listing("id0")
    assert listing.status == "seen"
    await db.close()


async def test_mark_seen_does_not_downgrade_saved(tmp_path):
    """mark_seen should NOT overwrite 'saved' status with 'seen'."""
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    await layer.mark_status("id0", "saved")
    await layer.mark_seen("id0")
    listing = await db.get_listing("id0")
    assert listing.status == "saved"
    await db.close()


async def test_mark_seen_is_idempotent(tmp_path):
    """Calling mark_seen twice should not raise or change status."""
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    await layer.mark_seen("id0")
    await layer.mark_seen("id0")
    listing = await db.get_listing("id0")
    assert listing.status == "seen"
    await db.close()


async def test_mark_seen_nonexistent_id_does_not_crash(tmp_path):
    """mark_seen on a missing ID should silently do nothing."""
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    await layer.mark_seen("nonexistent")  # must not raise
    await db.close()


async def test_get_last_source_poll_returns_most_recent(tmp_path):
    """get_last_source_poll should return the most recent last_polled across all sources."""
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    from datetime import timedelta
    now = datetime.now(timezone.utc)
    older = now - timedelta(minutes=30)
    newer = now - timedelta(minutes=5)
    await db.update_source_state("ebay", last_polled=older)
    await db.update_source_state("craigslist", last_polled=newer)
    last_poll = await layer.get_last_source_poll()
    assert last_poll is not None
    # Should be the newer timestamp (within a second tolerance)
    assert abs((last_poll - newer).total_seconds()) < 1
    await db.close()


async def test_get_last_source_poll_returns_none_when_no_sources(tmp_path):
    """get_last_source_poll returns None when no sources have been polled."""
    db = await make_db_with_listings(tmp_path)
    layer = DataLayer(db)
    last_poll = await layer.get_last_source_poll()
    assert last_poll is None
    await db.close()
