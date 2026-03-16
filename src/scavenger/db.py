import json
import aiosqlite
from datetime import datetime, timezone
from pathlib import Path
from scavenger.models import Listing

SCHEMA = """
PRAGMA journal_mode=WAL;

CREATE TABLE IF NOT EXISTS listings (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT DEFAULT '',
    price REAL,
    currency TEXT DEFAULT 'USD',
    condition TEXT,
    url TEXT NOT NULL,
    image_urls TEXT NOT NULL DEFAULT '[]',
    location TEXT,
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    relevance_score REAL NOT NULL DEFAULT 0.0,
    status TEXT NOT NULL DEFAULT 'new',
    ai_evaluation TEXT
);

CREATE TABLE IF NOT EXISTS price_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    listing_id TEXT NOT NULL REFERENCES listings(id),
    price REAL NOT NULL,
    observed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS image_cache (
    url_hash TEXT PRIMARY KEY,
    local_path TEXT NOT NULL,
    fetched_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sources (
    plugin_id TEXT PRIMARY KEY,
    last_polled TEXT,
    consecutive_errors INTEGER DEFAULT 0,
    rate_limit_until TEXT
);
"""


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _listing_to_row(listing: Listing) -> dict:
    return {
        "id": listing.id,
        "profile_id": listing.profile_id,
        "source_id": listing.source_id,
        "title": listing.title,
        "description": listing.description,
        "price": listing.price,
        "currency": listing.currency,
        "condition": listing.condition,
        "url": listing.url,
        "image_urls": json.dumps(listing.image_urls),
        "location": listing.location,
        "first_seen": listing.first_seen.isoformat(),
        "last_seen": listing.last_seen.isoformat(),
        "relevance_score": listing.relevance_score,
        "status": listing.status,
        "ai_evaluation": listing.ai_evaluation,
    }


def _row_to_listing(row: aiosqlite.Row) -> Listing:
    d = dict(row)
    d["image_urls"] = json.loads(d["image_urls"])
    return Listing(**d)


class Database:
    def __init__(self, path: Path):
        self.path = path
        self._conn: aiosqlite.Connection | None = None

    async def init(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        self._conn = await aiosqlite.connect(self.path)
        self._conn.row_factory = aiosqlite.Row
        await self._conn.executescript(SCHEMA)
        await self._conn.commit()

    async def migrate(self) -> None:
        """Apply schema migrations idempotently."""
        cursor = await self._conn.execute("PRAGMA table_info(listings)")
        columns = {row[1] for row in await cursor.fetchall()}
        if "ai_evaluation" not in columns:
            await self._conn.execute(
                "ALTER TABLE listings ADD COLUMN ai_evaluation TEXT"
            )
            await self._conn.commit()

    async def close(self) -> None:
        if self._conn:
            await self._conn.close()
            self._conn = None

    async def upsert_listing(self, listing: Listing) -> bool:
        """Returns True if listing is new."""
        row = _listing_to_row(listing)
        cursor = await self._conn.execute(
            """INSERT OR IGNORE INTO listings VALUES (
                :id, :profile_id, :source_id, :title, :description,
                :price, :currency, :condition, :url, :image_urls,
                :location, :first_seen, :last_seen, :relevance_score, :status,
                :ai_evaluation
            )""",
            row,
        )
        is_new = cursor.rowcount == 1
        if is_new:
            if listing.price is not None:
                await self._conn.execute(
                    "INSERT INTO price_history (listing_id, price, observed_at) VALUES (?, ?, ?)",
                    (listing.id, listing.price, _now_iso()),
                )
        else:
            await self._conn.execute(
                "UPDATE listings SET last_seen=? WHERE id=?",
                (listing.last_seen.isoformat(), listing.id),
            )
            # Check current price for history tracking
            cursor2 = await self._conn.execute(
                "SELECT price FROM listings WHERE id=?", (listing.id,)
            )
            existing_row = await cursor2.fetchone()
            if existing_row and listing.price is not None and listing.price != existing_row[0]:
                await self._conn.execute(
                    "INSERT INTO price_history (listing_id, price, observed_at) VALUES (?, ?, ?)",
                    (listing.id, listing.price, _now_iso()),
                )
        await self._conn.commit()
        return is_new

    async def get_listing(self, listing_id: str) -> Listing | None:
        cursor = await self._conn.execute(
            "SELECT * FROM listings WHERE id=?", (listing_id,)
        )
        row = await cursor.fetchone()
        return _row_to_listing(row) if row else None

    async def get_listings(
        self, status: str | None = None, profile_id: str | None = None, limit: int = 100
    ) -> list[Listing]:
        conditions, params = [], []
        if status:
            conditions.append("status=?")
            params.append(status)
        if profile_id:
            conditions.append("profile_id=?")
            params.append(profile_id)
        where = f" WHERE {' AND '.join(conditions)}" if conditions else ""
        params.append(limit)
        cursor = await self._conn.execute(
            f"SELECT * FROM listings{where} ORDER BY first_seen DESC LIMIT ?", params
        )
        return [_row_to_listing(r) for r in await cursor.fetchall()]

    async def update_listing_status(self, listing_id: str, status: str) -> None:
        """Update the status field of a listing by ID."""
        await self._conn.execute(
            "UPDATE listings SET status=? WHERE id=?",
            (status, listing_id),
        )
        await self._conn.commit()

    async def get_active_listings(
        self, profile_id: str | None = None, limit: int = 100
    ) -> list[Listing]:
        """Get listings excluding dismissed status, sorted by first_seen DESC."""
        conditions = ["status NOT IN ('dismissed', 'snoozed')"]
        params: list = []
        if profile_id:
            conditions.append("profile_id=?")
            params.append(profile_id)
        where = " WHERE " + " AND ".join(conditions)
        params.append(limit)
        cursor = await self._conn.execute(
            f"SELECT * FROM listings{where} ORDER BY first_seen DESC LIMIT ?",
            params,
        )
        return [_row_to_listing(r) for r in await cursor.fetchall()]

    async def get_price_history(self, listing_id: str) -> list[dict]:
        cursor = await self._conn.execute(
            "SELECT price, observed_at FROM price_history WHERE listing_id=? ORDER BY observed_at",
            (listing_id,),
        )
        return [dict(row) for row in await cursor.fetchall()]

    async def get_source_state(self, plugin_id: str) -> dict | None:
        cursor = await self._conn.execute(
            "SELECT * FROM sources WHERE plugin_id=?", (plugin_id,)
        )
        row = await cursor.fetchone()
        return dict(row) if row else None

    async def count_new_by_profile(self) -> dict[str, int]:
        """Return {profile_id: count} for listings with status='new'."""
        cursor = await self._conn.execute(
            "SELECT profile_id, COUNT(*) FROM listings WHERE status='new' GROUP BY profile_id"
        )
        return {row[0]: row[1] for row in await cursor.fetchall()}

    async def get_most_recent_poll(self) -> datetime | None:
        """Return the most recent last_polled timestamp across all sources."""
        cursor = await self._conn.execute(
            "SELECT MAX(last_polled) FROM sources WHERE last_polled IS NOT NULL"
        )
        row = await cursor.fetchone()
        if row and row[0]:
            return datetime.fromisoformat(row[0])
        return None

    async def update_source_state(
        self,
        plugin_id: str,
        last_polled: datetime | None = None,
        consecutive_errors: int = 0,
        rate_limit_until: datetime | None = None,
    ) -> None:
        await self._conn.execute(
            """INSERT INTO sources (plugin_id, last_polled, consecutive_errors, rate_limit_until)
               VALUES (?, ?, ?, ?)
               ON CONFLICT(plugin_id) DO UPDATE SET
                 last_polled=excluded.last_polled,
                 consecutive_errors=excluded.consecutive_errors,
                 rate_limit_until=excluded.rate_limit_until""",
            (
                plugin_id,
                last_polled.isoformat() if last_polled else None,
                consecutive_errors,
                rate_limit_until.isoformat() if rate_limit_until else None,
            ),
        )
        await self._conn.commit()
