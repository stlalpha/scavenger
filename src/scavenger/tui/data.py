import logging
from datetime import datetime, timedelta, timezone
from scavenger.db import Database
from scavenger.models import Listing

SNOOZE_DURATION = timedelta(hours=24)

logger = logging.getLogger(__name__)


class DataLayer:
    def __init__(self, db: Database) -> None:
        self._db = db

    async def get_listings(
        self, profile_id: str | None, limit: int = 100
    ) -> list[Listing]:
        await self._db.unsnooze_expired()
        return await self._db.get_active_listings(profile_id=profile_id, limit=limit)

    async def get_profile_stats(self) -> dict[str, int]:
        """Return {profile_id: new_listing_count}."""
        return await self._db.count_new_by_profile()

    async def mark_status(self, listing_id: str, status: str) -> None:
        listing = await self._db.get_listing(listing_id)
        if listing is None:
            return
        if status == "snoozed":
            until = datetime.now(timezone.utc) + SNOOZE_DURATION
            await self._db.snooze_listing(listing_id, until)
        else:
            await self._db.update_listing_status(listing_id, status)

    async def mark_seen(self, listing_id: str) -> None:
        """Transition a listing from 'new' to 'seen'. Does not downgrade other statuses."""
        listing = await self._db.get_listing(listing_id)
        if listing is None:
            return
        if listing.status == "new":
            await self._db.update_listing_status(listing_id, "seen")

    async def get_last_source_poll(self) -> datetime | None:
        """Return the most recent last_polled timestamp across all sources."""
        return await self._db.get_most_recent_poll()

    async def get_source_states(self) -> list[dict]:
        """Return per-source poll state."""
        return await self._db.get_all_source_states()
