import logging
from scavenger.db import Database
from scavenger.models import Listing

logger = logging.getLogger(__name__)

EXCLUDED_STATUSES = ("dismissed",)


class DataLayer:
    def __init__(self, db: Database) -> None:
        self._db = db

    async def get_listings(
        self, profile_id: str | None, limit: int = 100
    ) -> list[Listing]:
        all_listings = await self._db.get_listings(
            profile_id=profile_id, limit=limit * 4
        )
        return [
            l for l in all_listings
            if l.status not in EXCLUDED_STATUSES
        ][:limit]

    async def get_profile_stats(self) -> dict[str, int]:
        """Return {profile_id: new_listing_count}."""
        listings = await self._db.get_listings(status="new", limit=1000)
        stats: dict[str, int] = {}
        for listing in listings:
            stats[listing.profile_id] = stats.get(listing.profile_id, 0) + 1
        return stats

    async def mark_status(self, listing_id: str, status: str) -> None:
        listing = await self._db.get_listing(listing_id)
        if listing is None:
            return
        await self._db._conn.execute(
            "UPDATE listings SET status=? WHERE id=?",
            (status, listing_id),
        )
        await self._db._conn.commit()
