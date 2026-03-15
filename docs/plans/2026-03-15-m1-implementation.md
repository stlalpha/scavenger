# M1: Core Engine Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the SCAVENGER core engine: project scaffold, config parsing, SQLite persistence, content-hash dedup, relevance scoring, plugin protocol, eBay + Craigslist plugins, daemon scheduler + Unix socket IPC, and scavenger-ctl CLI.

**Architecture:** Asyncio daemon with APScheduler (one job per profile), plugins return `list[Listing]`, daemon deduplicates via SHA-256(normalized_url), scores, and upserts to SQLite. `scavenger-ctl` sends JSON commands over a Unix domain socket.

**Tech Stack:** Python 3.12+, uv, pydantic v2, aiosqlite, httpx, apscheduler, click, beautifulsoup4, pytest + pytest-asyncio + respx

**Working directory for all commands:** `.worktrees/m1-core-engine/` inside the repo root.

---

### Task 1: Project scaffolding

**Files:**
- Create: `pyproject.toml`
- Create: `src/scavenger/__init__.py`
- Create: `src/scavenger/main.py`
- Create: `src/scavenger/plugins/__init__.py`
- Create: `src/scavenger/daemon/__init__.py`
- Create: `tests/__init__.py`

**Step 1: Create `pyproject.toml`**

```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "scavenger"
version = "0.1.0"
description = "Continuous web intelligence terminal"
requires-python = ">=3.12"
dependencies = [
    "pydantic>=2.0",
    "aiosqlite>=0.20",
    "httpx>=0.27",
    "apscheduler>=3.10",
    "click>=8.1",
    "beautifulsoup4>=4.12",
    "tomli-w>=1.0",
]

[project.optional-dependencies]
dev = [
    "pytest>=8.0",
    "pytest-asyncio>=0.23",
    "respx>=0.21",
]

[project.scripts]
scavenger = "scavenger.main:cli"
scavenger-ctl = "scavenger.ctl:cli"

[tool.hatch.build.targets.wheel]
packages = ["src/scavenger"]

[tool.pytest.ini_options]
asyncio_mode = "auto"
testpaths = ["tests"]
pythonpath = ["src"]
```

**Step 2: Create package skeleton**

```bash
mkdir -p src/scavenger/plugins src/scavenger/daemon tests
touch src/scavenger/__init__.py
touch src/scavenger/plugins/__init__.py
touch src/scavenger/daemon/__init__.py
touch tests/__init__.py
```

**Step 3: Create `src/scavenger/main.py`**

```python
import click

@click.command()
def cli():
    """SCAVENGER — TUI coming in M2."""
    click.echo("TUI coming in M2. Use scavenger-ctl to control the daemon.")
```

**Step 4: Install and verify**

```bash
uv sync --extra dev
uv run python -c "import scavenger; print('ok')"
```
Expected: `ok`

**Step 5: Commit**

```bash
git add pyproject.toml src/ tests/
git commit -m "feat: scaffold project structure"
```

---

### Task 2: Pydantic models

**Files:**
- Create: `src/scavenger/models.py`
- Create: `tests/test_models.py`

**Step 1: Write failing tests**

```python
# tests/test_models.py
from datetime import datetime, timezone
import pytest
from scavenger.models import Listing, Profile

def test_listing_defaults():
    now = datetime.now(timezone.utc)
    listing = Listing(
        id="abc123",
        profile_id="p1",
        source_id="ebay",
        title="Sony 85mm f/1.4",
        url="https://www.ebay.com/itm/123",
        first_seen=now,
        last_seen=now,
        relevance_score=75.0,
    )
    assert listing.status == "new"
    assert listing.currency == "USD"
    assert listing.price is None

def test_listing_requires_id():
    with pytest.raises(Exception):
        Listing(title="oops")

def test_profile_keyword_structure():
    profile = Profile(
        id="p1",
        name="Sony Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken"],
        sources=["ebay"],
    )
    assert profile.poll_interval_sec == 900
    assert profile.enabled is True
    assert profile.alert_priority == "normal"

def test_profile_invalid_alert_priority():
    with pytest.raises(Exception):
        Profile(
            id="p1",
            name="Bad",
            keywords=["test"],
            negative_keywords=[],
            sources=["ebay"],
            alert_priority="invalid",
        )

def test_profile_poll_interval_minimum():
    with pytest.raises(Exception):
        Profile(
            id="p1",
            name="Fast",
            keywords=["test"],
            negative_keywords=[],
            sources=["ebay"],
            poll_interval_sec=5,
        )
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_models.py -v
```

**Step 3: Implement `src/scavenger/models.py`**

```python
from datetime import datetime
from typing import Literal
from pydantic import BaseModel, field_validator


class Listing(BaseModel):
    id: str
    profile_id: str
    source_id: str
    title: str
    description: str = ""
    price: float | None = None
    currency: str = "USD"
    condition: str | None = None
    url: str
    image_urls: list[str] = []
    location: str | None = None
    first_seen: datetime
    last_seen: datetime
    relevance_score: float = 0.0
    status: str = "new"


class Profile(BaseModel):
    id: str
    name: str
    keywords: list[str | list[str]]
    negative_keywords: list[str] = []
    sources: list[str]
    price_min: float | None = None
    price_max: float | None = None
    poll_interval_sec: int = 900
    alert_priority: Literal["high", "normal", "low"] = "normal"
    enabled: bool = True
    tags: list[str] = []
    escalation_keywords: list[str] = []
    location_radius_mi: int | None = None

    @field_validator("poll_interval_sec")
    @classmethod
    def poll_interval_must_be_positive(cls, v: int) -> int:
        if v < 30:
            raise ValueError("poll_interval_sec must be >= 30")
        return v
```

**Step 4: Run — expect 5 passed**

```bash
uv run pytest tests/test_models.py -v
```

**Step 5: Commit**

```bash
git add src/scavenger/models.py tests/test_models.py
git commit -m "feat: add Listing and Profile Pydantic models"
```

---

### Task 3: Config parsing

**Files:**
- Create: `src/scavenger/config.py`
- Create: `tests/test_config.py`
- Create: `tests/fixtures/valid_config.toml`
- Create: `tests/fixtures/invalid_config.toml`

**Step 1: Create fixtures**

`tests/fixtures/valid_config.toml`:
```toml
[global]
db_path = "~/.local/share/scavenger/scavenger.db"
log_level = "INFO"

[[profiles]]
id = "sony-glass"
name = "Sony A-mount Glass"
keywords = ["sony", ["a-mount", "alpha mount"]]
negative_keywords = ["broken", "fungus"]
sources = ["ebay", "usedphotopro"]
price_min = 50.0
price_max = 800.0
alert_priority = "high"
poll_interval_sec = 900
enabled = true

[[profiles]]
id = "as400"
name = "IBM AS/400"
keywords = [["as/400", "as400", "iseries"], "ibm"]
negative_keywords = ["parts only"]
sources = ["ebay", "craigslist"]
alert_priority = "high"
poll_interval_sec = 1800
enabled = true
```

`tests/fixtures/invalid_config.toml`:
```toml
[[profiles]]
name = "Missing ID"
keywords = ["test"]
sources = ["ebay"]
alert_priority = "bogus_priority"
```

**Step 2: Write failing tests**

```python
# tests/test_config.py
import pytest
from pathlib import Path
from scavenger.config import load_config, ConfigError

FIXTURES = Path(__file__).parent / "fixtures"

def test_load_valid_config():
    config = load_config(FIXTURES / "valid_config.toml")
    assert len(config.profiles) == 2
    assert config.profiles[0].name == "Sony A-mount Glass"
    assert config.profiles[0].price_max == 800.0

def test_profile_keywords_preserved():
    config = load_config(FIXTURES / "valid_config.toml")
    profile = config.profiles[0]
    assert "sony" in profile.keywords
    assert ["a-mount", "alpha mount"] in profile.keywords

def test_load_invalid_config_raises():
    with pytest.raises(ConfigError):
        load_config(FIXTURES / "invalid_config.toml")

def test_missing_file_raises():
    with pytest.raises(ConfigError):
        load_config(Path("/nonexistent/config.toml"))

def test_db_path_is_expanded():
    config = load_config(FIXTURES / "valid_config.toml")
    assert not str(config.db_path).startswith("~")
```

**Step 3: Run — expect ImportError**

```bash
uv run pytest tests/test_config.py -v
```

**Step 4: Implement `src/scavenger/config.py`**

```python
import tomllib
from pathlib import Path
from pydantic import BaseModel, ValidationError
from scavenger.models import Profile


class GlobalConfig(BaseModel):
    db_path: str = "~/.local/share/scavenger/scavenger.db"
    image_cache_path: str = "~/.cache/scavenger/images"
    log_level: str = "INFO"
    socket_path: str = "~/.run/scavenger/daemon.sock"


class AppConfig(BaseModel):
    global_config: GlobalConfig = GlobalConfig()
    profiles: list[Profile]

    @property
    def log_level(self) -> str:
        return self.global_config.log_level

    @property
    def db_path(self) -> Path:
        return Path(self.global_config.db_path).expanduser()

    @property
    def socket_path(self) -> Path:
        return Path(self.global_config.socket_path).expanduser()


class ConfigError(Exception):
    pass


def load_config(path: Path) -> AppConfig:
    try:
        raw = path.read_text()
    except FileNotFoundError:
        raise ConfigError(f"Config file not found: {path}")
    except OSError as e:
        raise ConfigError(f"Cannot read config: {e}")

    try:
        data = tomllib.loads(raw)
    except tomllib.TOMLDecodeError as e:
        raise ConfigError(f"Invalid TOML: {e}")

    try:
        global_data = data.get("global", {})
        profiles_data = data.get("profiles", [])
        global_config = GlobalConfig(**global_data)
        profiles = [Profile(**p) for p in profiles_data]
        return AppConfig(global_config=global_config, profiles=profiles)
    except (ValidationError, TypeError) as e:
        raise ConfigError(f"Invalid config: {e}")
```

**Step 5: Create fixtures directory**

```bash
mkdir -p tests/fixtures
```

(Write the two TOML files from Step 1.)

**Step 6: Run — expect 5 passed**

```bash
uv run pytest tests/test_config.py -v
```

**Step 7: Commit**

```bash
git add src/scavenger/config.py tests/test_config.py tests/fixtures/
git commit -m "feat: add TOML config parsing with Pydantic validation"
```

---

### Task 4: SQLite schema and database helpers

**Files:**
- Create: `src/scavenger/db.py`
- Create: `tests/test_db.py`

**Step 1: Write failing tests**

```python
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
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_db.py -v
```

**Step 3: Implement `src/scavenger/db.py`**

```python
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
    status TEXT NOT NULL DEFAULT 'new'
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
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._conn = await aiosqlite.connect(self.path)
        self._conn.row_factory = aiosqlite.Row
        await self._conn.executescript(SCHEMA)
        await self._conn.commit()

    async def close(self) -> None:
        if self._conn:
            await self._conn.close()

    async def upsert_listing(self, listing: Listing) -> bool:
        """Returns True if listing is new."""
        existing = await self.get_listing(listing.id)
        if existing is None:
            row = _listing_to_row(listing)
            await self._conn.execute(
                """INSERT INTO listings VALUES (
                    :id, :profile_id, :source_id, :title, :description,
                    :price, :currency, :condition, :url, :image_urls,
                    :location, :first_seen, :last_seen, :relevance_score, :status
                )""",
                row,
            )
            if listing.price is not None:
                await self._conn.execute(
                    "INSERT INTO price_history (listing_id, price, observed_at) VALUES (?, ?, ?)",
                    (listing.id, listing.price, _now_iso()),
                )
            await self._conn.commit()
            return True
        else:
            await self._conn.execute(
                "UPDATE listings SET last_seen=? WHERE id=?",
                (listing.last_seen.isoformat(), listing.id),
            )
            if listing.price is not None and listing.price != existing.price:
                await self._conn.execute(
                    "INSERT INTO price_history (listing_id, price, observed_at) VALUES (?, ?, ?)",
                    (listing.id, listing.price, _now_iso()),
                )
            await self._conn.commit()
            return False

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

    async def get_price_history(self, listing_id: str) -> list[dict]:
        cursor = await self._conn.execute(
            "SELECT price, observed_at FROM price_history WHERE listing_id=? ORDER BY observed_at",
            (listing_id,),
        )
        return [dict(row) for row in await cursor.fetchall()]

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
```

**Step 4: Run — expect 6 passed**

```bash
uv run pytest tests/test_db.py -v
```

**Step 5: Commit**

```bash
git add src/scavenger/db.py tests/test_db.py
git commit -m "feat: add SQLite schema and Database helpers"
```

---

### Task 5: Deduplication

**Files:**
- Create: `src/scavenger/dedup.py`
- Create: `tests/test_dedup.py`

**Step 1: Write failing tests**

```python
# tests/test_dedup.py
from scavenger.dedup import normalize_url, content_hash

def test_normalize_strips_utm_params():
    url = "https://www.ebay.com/itm/123?utm_source=newsletter&utm_medium=email"
    assert normalize_url(url) == "https://www.ebay.com/itm/123"

def test_normalize_strips_ebay_tracking():
    url = "https://www.ebay.com/itm/123?ssPageName=STRK&_trkparms=aid%3D111001"
    assert normalize_url(url) == "https://www.ebay.com/itm/123"

def test_normalize_removes_fragment():
    url = "https://ebay.com/itm/123#description"
    assert "#" not in normalize_url(url)

def test_content_hash_is_stable():
    url = "https://www.ebay.com/itm/123456789"
    assert content_hash(url) == content_hash(url)

def test_content_hash_differs_for_different_urls():
    assert content_hash("https://ebay.com/itm/111") != content_hash("https://ebay.com/itm/222")

def test_content_hash_is_64_char_hex():
    h = content_hash("https://example.com/item/1")
    assert len(h) == 64
    assert all(c in "0123456789abcdef" for c in h)
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_dedup.py -v
```

**Step 3: Implement `src/scavenger/dedup.py`**

```python
import hashlib
from urllib.parse import urlparse, urlencode, parse_qsl, urlunparse

STRIP_PARAMS = frozenset({
    "utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content",
    "ssPageName", "_trkparms", "_trktoken", "hash", "ref",
    "mkevt", "mkcid", "mkrid", "campid", "toolid",
})


def normalize_url(url: str) -> str:
    parsed = urlparse(url)
    filtered = [
        (k, v) for k, v in parse_qsl(parsed.query)
        if k not in STRIP_PARAMS and not k.startswith("utm_")
    ]
    return urlunparse(parsed._replace(query=urlencode(filtered), fragment=""))


def content_hash(url: str) -> str:
    return hashlib.sha256(normalize_url(url).encode()).hexdigest()
```

**Step 4: Run — expect 6 passed**

```bash
uv run pytest tests/test_dedup.py -v
```

**Step 5: Commit**

```bash
git add src/scavenger/dedup.py tests/test_dedup.py
git commit -m "feat: add URL normalization and content-hash deduplication"
```

---

### Task 6: Relevance scoring

**Files:**
- Create: `src/scavenger/scoring.py`
- Create: `tests/test_scoring.py`

**Step 1: Write failing tests**

```python
# tests/test_scoring.py
import pytest
from scavenger.scoring import score_listing
from scavenger.models import Profile


@pytest.fixture
def sony_profile():
    return Profile(
        id="sony", name="Sony Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken", "fungus", "parts only"],
        sources=["ebay"],
        price_min=50.0, price_max=800.0,
    )


def test_strong_title_match_scores_high(sony_profile):
    score = score_listing(sony_profile, "Sony A-mount 85mm f/1.4 lens", "Great condition", 250.0)
    assert score >= 70.0

def test_negative_keyword_scores_zero(sony_profile):
    score = score_listing(sony_profile, "Sony A-mount lens broken", "parts only", 50.0)
    assert score == 0.0

def test_no_keyword_match_scores_zero(sony_profile):
    score = score_listing(sony_profile, "Canon EF 50mm lens", "Great Canon lens", 200.0)
    assert score == 0.0

def test_price_outside_band_reduces_score(sony_profile):
    in_band = score_listing(sony_profile, "Sony A-mount lens", "", 300.0)
    out_of_band = score_listing(sony_profile, "Sony A-mount lens", "", 2000.0)
    assert in_band > out_of_band

def test_title_match_scores_higher_than_description_only(sony_profile):
    title_match = score_listing(sony_profile, "Sony A-mount 85mm", "", 300.0)
    desc_match = score_listing(sony_profile, "Camera lens for sale", "Sony A-mount 85mm", 300.0)
    assert title_match > desc_match
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_scoring.py -v
```

**Step 3: Implement `src/scavenger/scoring.py`**

```python
import re
from scavenger.models import Profile


def _contains(text: str, term: str) -> bool:
    return bool(re.search(re.escape(term), text, re.IGNORECASE))


def _group_matches(text: str, keyword: str | list[str]) -> bool:
    if isinstance(keyword, list):
        return any(_contains(text, term) for term in keyword)
    return _contains(text, keyword)


def score_listing(
    profile: Profile, title: str, description: str, price: float | None
) -> float:
    combined = f"{title} {description}"

    for neg in profile.negative_keywords:
        if _contains(combined, neg):
            return 0.0

    total = len(profile.keywords)
    if total == 0:
        return 0.0

    combined_hits = sum(1 for kw in profile.keywords if _group_matches(combined, kw))
    if combined_hits < total:
        return 0.0

    title_hits = sum(1 for kw in profile.keywords if _group_matches(title, kw))
    desc_hits = sum(1 for kw in profile.keywords if _group_matches(description, kw))

    title_score = (title_hits / total) * 40.0
    extra_desc = max(0, desc_hits - title_hits)
    desc_score = (min(extra_desc, total) / total) * 20.0

    price_score = 0.0
    if price is None or (profile.price_min is None and profile.price_max is None):
        price_score = 20.0
    elif (profile.price_min is None or price >= profile.price_min) and \
         (profile.price_max is None or price <= profile.price_max):
        price_score = 20.0

    return min(100.0, title_score + desc_score + price_score + 20.0)
```

**Step 4: Run — expect 5 passed**

```bash
uv run pytest tests/test_scoring.py -v
```

**Step 5: Commit**

```bash
git add src/scavenger/scoring.py tests/test_scoring.py
git commit -m "feat: add relevance scoring with AND-of-OR keyword logic"
```

---

### Task 7: Plugin Protocol

**Files:**
- Create: `src/scavenger/plugins/base.py`
- Create: `tests/test_plugin_base.py`

**Step 1: Write failing tests**

```python
# tests/test_plugin_base.py
from scavenger.plugins.base import Plugin
from scavenger.models import Profile, Listing


class GoodPlugin:
    plugin_id = "good"
    async def fetch(self, profile: Profile) -> list[Listing]: return []
    async def supports_geo(self) -> bool: return False


class BadPlugin:
    pass


def test_good_plugin_satisfies_protocol():
    assert isinstance(GoodPlugin(), Plugin)

def test_bad_plugin_fails_protocol():
    assert not isinstance(BadPlugin(), Plugin)
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_plugin_base.py -v
```

**Step 3: Implement `src/scavenger/plugins/base.py`**

```python
from typing import Protocol, runtime_checkable
from scavenger.models import Profile, Listing


@runtime_checkable
class Plugin(Protocol):
    plugin_id: str

    async def fetch(self, profile: Profile) -> list[Listing]: ...
    async def supports_geo(self) -> bool: ...
```

**Step 4: Run — expect 2 passed**

```bash
uv run pytest tests/test_plugin_base.py -v
```

**Step 5: Commit**

```bash
git add src/scavenger/plugins/base.py tests/test_plugin_base.py
git commit -m "feat: add runtime-checkable Plugin Protocol"
```

---

### Task 8: eBay plugin

**Files:**
- Create: `src/scavenger/plugins/ebay.py`
- Create: `tests/test_plugin_ebay.py`
- Create: `tests/fixtures/ebay_rss.xml`

**Step 1: Create `tests/fixtures/ebay_rss.xml`**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>eBay Search Results</title>
    <item>
      <title><![CDATA[Sony 85mm f/1.4 A-mount Lens - Excellent]]></title>
      <link>https://www.ebay.com/itm/123456789012</link>
      <description><![CDATA[Beautiful Sony A-mount 85mm f/1.4. No fungus. $249.99]]></description>
      <pubDate>Sun, 15 Mar 2026 10:00:00 +0000</pubDate>
      <enclosure url="https://i.ebayimg.com/images/g/abc/s-l500.jpg" type="image/jpeg"/>
    </item>
    <item>
      <title><![CDATA[Sony A-mount 50mm f/1.4 Zeiss]]></title>
      <link>https://www.ebay.com/itm/987654321098</link>
      <description><![CDATA[Sony Zeiss 50mm SSM. $399.00]]></description>
      <pubDate>Sun, 15 Mar 2026 09:00:00 +0000</pubDate>
    </item>
  </channel>
</rss>
```

**Step 2: Write failing tests**

```python
# tests/test_plugin_ebay.py
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
```

**Step 3: Run — expect ImportError**

```bash
uv run pytest tests/test_plugin_ebay.py -v
```

**Step 4: Implement `src/scavenger/plugins/ebay.py`**

```python
import re
import logging
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from xml.etree import ElementTree as ET

import httpx

from scavenger.dedup import content_hash
from scavenger.models import Listing, Profile

logger = logging.getLogger(__name__)
RSS_URL = "https://rss.ebay.com/rss2/search"
PRICE_RE = re.compile(r"\$([0-9,]+(?:\.[0-9]{2})?)")


def _extract_price(text: str) -> float | None:
    m = PRICE_RE.search(text)
    return float(m.group(1).replace(",", "")) if m else None


class EbayPlugin:
    plugin_id = "ebay"

    async def fetch(self, profile: Profile) -> list[Listing]:
        keywords = " ".join(
            kw if isinstance(kw, str) else " ".join(kw) for kw in profile.keywords
        )
        try:
            async with httpx.AsyncClient(timeout=30.0) as client:
                resp = await client.get(RSS_URL, params={"kw": keywords, "country": "us", "siteid": "0"})
                resp.raise_for_status()
        except (httpx.HTTPError, httpx.TimeoutException) as e:
            logger.warning("eBay fetch failed: %s", e)
            return []
        return self._parse(resp.content, profile)

    def _parse(self, content: bytes, profile: Profile) -> list[Listing]:
        try:
            root = ET.fromstring(content)
        except ET.ParseError:
            return []
        channel = root.find("channel")
        if channel is None:
            return []
        now = datetime.now(timezone.utc)
        listings = []
        for item in channel.findall("item"):
            title_el, link_el = item.find("title"), item.find("link")
            if title_el is None or link_el is None:
                continue
            title = title_el.text or ""
            url = link_el.text or ""
            desc_el = item.find("description")
            description = desc_el.text or "" if desc_el is not None else ""
            pub_el = item.find("pubDate")
            try:
                pub_date = parsedate_to_datetime(pub_el.text) if pub_el is not None and pub_el.text else now
            except Exception:
                pub_date = now
            enclosure = item.find("enclosure")
            image_urls = [enclosure.get("url")] if enclosure is not None and enclosure.get("url") else []
            listings.append(Listing(
                id=content_hash(url),
                profile_id=profile.id,
                source_id=self.plugin_id,
                title=title,
                description=description,
                price=_extract_price(description),
                url=url,
                image_urls=image_urls,
                first_seen=pub_date,
                last_seen=now,
                relevance_score=0.0,
            ))
        return listings

    async def supports_geo(self) -> bool:
        return False
```

**Step 5: Run — expect 6 passed**

```bash
uv run pytest tests/test_plugin_ebay.py -v
```

**Step 6: Commit**

```bash
git add src/scavenger/plugins/ebay.py tests/test_plugin_ebay.py tests/fixtures/ebay_rss.xml
git commit -m "feat: add eBay RSS source plugin"
```

---

### Task 9: Craigslist plugin

**Files:**
- Create: `src/scavenger/plugins/craigslist.py`
- Create: `tests/test_plugin_craigslist.py`
- Create: `tests/fixtures/craigslist_rss.xml`

**Step 1: Create `tests/fixtures/craigslist_rss.xml`**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>SF Bay - electronics</title>
    <item>
      <title><![CDATA[Sony A-mount 70-200mm f/2.8 G SSM - $650]]></title>
      <link>https://sfbay.craigslist.org/eby/ele/d/7890123456.html</link>
      <description><![CDATA[Sony A-mount 70-200mm f/2.8 G SSM. Excellent condition. Oakland.]]></description>
      <pubDate>Sun, 15 Mar 2026 08:00:00 +0000</pubDate>
      <enclosure url="https://images.craigslist.org/00d0d_abc_600x450.jpg" type="image/jpeg" length="0"/>
    </item>
    <item>
      <title><![CDATA[Sigma A-mount 35mm f/1.4 - $200]]></title>
      <link>https://sfbay.craigslist.org/eby/ele/d/1234567890.html</link>
      <description><![CDATA[Sigma 35mm f/1.4 Art for Sony A-mount. Minor wear. $200.]]></description>
      <pubDate>Sun, 15 Mar 2026 07:00:00 +0000</pubDate>
    </item>
  </channel>
</rss>
```

**Step 2: Write failing tests**

```python
# tests/test_plugin_craigslist.py
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
```

**Step 3: Run — expect ImportError**

```bash
uv run pytest tests/test_plugin_craigslist.py -v
```

**Step 4: Implement `src/scavenger/plugins/craigslist.py`**

```python
import re
import logging
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from xml.etree import ElementTree as ET

import httpx

from scavenger.dedup import content_hash
from scavenger.models import Listing, Profile

logger = logging.getLogger(__name__)
DEFAULT_CITIES = ["sfbay", "newyork", "losangeles", "chicago", "seattle"]
PRICE_RE = re.compile(r"\$([0-9,]+(?:\.[0-9]{2})?)")


def _extract_price(text: str) -> float | None:
    m = PRICE_RE.search(text)
    return float(m.group(1).replace(",", "")) if m else None


class CraigslistPlugin:
    plugin_id = "craigslist"

    def __init__(self, cities: list[str] | None = None):
        self._cities = cities or DEFAULT_CITIES

    async def fetch(self, profile: Profile) -> list[Listing]:
        keywords = " ".join(
            kw if isinstance(kw, str) else " ".join(kw) for kw in profile.keywords
        )
        listings = []
        for city in self._cities:
            listings.extend(await self._fetch_city(city, keywords, profile))
        return listings

    async def _fetch_city(self, city: str, keywords: str, profile: Profile) -> list[Listing]:
        url = f"https://{city}.craigslist.org/search/sss"
        try:
            async with httpx.AsyncClient(timeout=30.0) as client:
                resp = await client.get(url, params={"query": keywords, "format": "rss"})
                resp.raise_for_status()
        except (httpx.HTTPError, httpx.TimeoutException) as e:
            logger.warning("Craigslist fetch failed for %s: %s", city, e)
            return []
        return self._parse(resp.content, profile, city)

    def _parse(self, content: bytes, profile: Profile, city: str) -> list[Listing]:
        try:
            root = ET.fromstring(content)
        except ET.ParseError:
            return []
        channel = root.find("channel")
        if channel is None:
            return []
        now = datetime.now(timezone.utc)
        listings = []
        for item in channel.findall("item"):
            title_el, link_el = item.find("title"), item.find("link")
            if title_el is None or link_el is None:
                continue
            title = title_el.text or ""
            url = link_el.text or ""
            desc_el = item.find("description")
            description = desc_el.text or "" if desc_el is not None else ""
            pub_el = item.find("pubDate")
            try:
                pub_date = parsedate_to_datetime(pub_el.text) if pub_el is not None and pub_el.text else now
            except Exception:
                pub_date = now
            enclosure = item.find("enclosure")
            image_urls = [enclosure.get("url")] if enclosure is not None and enclosure.get("url") else []
            listings.append(Listing(
                id=content_hash(url),
                profile_id=profile.id,
                source_id=self.plugin_id,
                title=title,
                description=description,
                price=_extract_price(title) or _extract_price(description),
                location=city,
                url=url,
                image_urls=image_urls,
                first_seen=pub_date,
                last_seen=now,
                relevance_score=0.0,
            ))
        return listings

    async def supports_geo(self) -> bool:
        return True
```

**Step 5: Run — expect 6 passed**

```bash
uv run pytest tests/test_plugin_craigslist.py -v
```

**Step 6: Commit**

```bash
git add src/scavenger/plugins/craigslist.py tests/test_plugin_craigslist.py tests/fixtures/craigslist_rss.xml
git commit -m "feat: add Craigslist RSS source plugin with multi-city support"
```

---

### Task 10: Daemon scheduler

**Files:**
- Create: `src/scavenger/daemon/scheduler.py`
- Create: `tests/test_scheduler.py`

**Step 1: Write failing tests**

```python
# tests/test_scheduler.py
import asyncio
import pytest
from unittest.mock import AsyncMock
from scavenger.daemon.scheduler import PollScheduler
from scavenger.models import Profile


def make_profile(**overrides) -> Profile:
    return Profile(**{
        "id": "test", "name": "Test", "keywords": ["test"],
        "negative_keywords": [], "sources": ["ebay"],
        "poll_interval_sec": 60, "enabled": True, **overrides
    })


async def test_scheduler_starts_and_stops():
    s = PollScheduler()
    await s.start()
    assert s.running
    await s.stop()
    assert not s.running


async def test_add_profile_creates_job():
    s = PollScheduler()
    await s.start()
    s.add_profile(make_profile(), callback=AsyncMock())
    assert s.has_job("test")
    await s.stop()


async def test_disabled_profile_not_scheduled():
    s = PollScheduler()
    await s.start()
    s.add_profile(make_profile(enabled=False), callback=AsyncMock())
    assert not s.has_job("test")
    await s.stop()


async def test_remove_profile_removes_job():
    s = PollScheduler()
    await s.start()
    s.add_profile(make_profile(), callback=AsyncMock())
    s.remove_profile("test")
    assert not s.has_job("test")
    await s.stop()


async def test_trigger_now_calls_callback():
    s = PollScheduler()
    await s.start()
    mock_cb = AsyncMock(return_value=[])
    profile = make_profile()
    s.add_profile(profile, callback=mock_cb)
    await s.trigger_now("test")
    await asyncio.sleep(0.05)
    mock_cb.assert_called_once_with(profile)
    await s.stop()
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_scheduler.py -v
```

**Step 3: Implement `src/scavenger/daemon/scheduler.py`**

```python
import asyncio
import logging
import random
from typing import Callable, Awaitable

from apscheduler.schedulers.asyncio import AsyncIOScheduler
from scavenger.models import Profile, Listing

logger = logging.getLogger(__name__)
PollCallback = Callable[[Profile], Awaitable[list[Listing]]]


class PollScheduler:
    def __init__(self):
        self._scheduler = AsyncIOScheduler()
        self._callbacks: dict[str, PollCallback] = {}
        self._profiles: dict[str, Profile] = {}

    @property
    def running(self) -> bool:
        return self._scheduler.running

    async def start(self) -> None:
        self._scheduler.start()

    async def stop(self) -> None:
        self._scheduler.shutdown(wait=False)

    def add_profile(self, profile: Profile, callback: PollCallback) -> None:
        if not profile.enabled:
            return
        self._callbacks[profile.id] = callback
        self._profiles[profile.id] = profile
        interval = int(profile.poll_interval_sec * (1 + random.uniform(-0.1, 0.1)))
        self._scheduler.add_job(
            self._run_poll, "interval", seconds=interval,
            args=[profile.id], id=profile.id, replace_existing=True,
        )

    def remove_profile(self, profile_id: str) -> None:
        if self._scheduler.get_job(profile_id):
            self._scheduler.remove_job(profile_id)
        self._callbacks.pop(profile_id, None)
        self._profiles.pop(profile_id, None)

    def has_job(self, profile_id: str) -> bool:
        return self._scheduler.get_job(profile_id) is not None

    async def trigger_now(self, profile_id: str) -> None:
        if profile_id not in self._callbacks:
            logger.warning("No callback for profile: %s", profile_id)
            return
        asyncio.create_task(self._run_poll(profile_id))

    async def _run_poll(self, profile_id: str) -> None:
        callback = self._callbacks.get(profile_id)
        profile = self._profiles.get(profile_id)
        if not callback or not profile:
            return
        try:
            await callback(profile)
        except Exception:
            logger.exception("Poll failed for profile: %s", profile_id)
```

**Step 4: Run — expect 5 passed**

```bash
uv run pytest tests/test_scheduler.py -v
```

**Step 5: Commit**

```bash
git add src/scavenger/daemon/scheduler.py tests/test_scheduler.py
git commit -m "feat: add APScheduler-backed poll scheduler with jitter"
```

---

### Task 11: Unix socket server

**Files:**
- Create: `src/scavenger/daemon/socket_server.py`
- Create: `tests/test_socket_server.py`

**Step 1: Write failing tests**

```python
# tests/test_socket_server.py
import asyncio
import json
import pytest
from pathlib import Path
from scavenger.daemon.socket_server import SocketServer


@pytest.fixture
async def server(tmp_path):
    sock_path = tmp_path / "test.sock"
    srv = SocketServer(sock_path)
    await srv.start()
    yield srv, sock_path
    await srv.stop()


async def send_command(sock_path: Path, command: dict) -> dict:
    reader, writer = await asyncio.open_unix_connection(str(sock_path))
    writer.write(json.dumps(command).encode() + b"\n")
    await writer.drain()
    response = await reader.readline()
    writer.close()
    await writer.wait_closed()
    return json.loads(response)


async def test_status_command(server):
    srv, sock_path = server
    resp = await send_command(sock_path, {"command": "status"})
    assert resp["status"] == "ok"


async def test_unknown_command_returns_error(server):
    srv, sock_path = server
    resp = await send_command(sock_path, {"command": "bogus"})
    assert resp["status"] == "error"


async def test_poll_command_triggers_handler(server):
    srv, sock_path = server
    called = []
    async def handler(profile_id: str): called.append(profile_id)
    srv.register_poll_handler(handler)
    await send_command(sock_path, {"command": "poll", "profile_id": "sony"})
    await asyncio.sleep(0.05)
    assert "sony" in called
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_socket_server.py -v
```

**Step 3: Implement `src/scavenger/daemon/socket_server.py`**

```python
import asyncio
import json
import logging
from pathlib import Path
from typing import Callable, Awaitable

logger = logging.getLogger(__name__)
PollHandler = Callable[[str], Awaitable[None]]


class SocketServer:
    def __init__(self, socket_path: Path):
        self._path = socket_path
        self._server: asyncio.Server | None = None
        self._poll_handler: PollHandler | None = None

    def register_poll_handler(self, handler: PollHandler) -> None:
        self._poll_handler = handler

    async def start(self) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        if self._path.exists():
            self._path.unlink()
        self._server = await asyncio.start_unix_server(self._handle, path=str(self._path))

    async def stop(self) -> None:
        if self._server:
            self._server.close()
            await self._server.wait_closed()
        if self._path.exists():
            self._path.unlink()

    async def _handle(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        try:
            line = await reader.readline()
            if not line:
                return
            try:
                request = json.loads(line)
            except json.JSONDecodeError:
                response = {"status": "error", "message": "invalid JSON"}
            else:
                response = await self._dispatch(request)
            writer.write(json.dumps(response).encode() + b"\n")
            await writer.drain()
        except Exception as e:
            logger.exception("Socket error: %s", e)
        finally:
            writer.close()
            await writer.wait_closed()

    async def _dispatch(self, request: dict) -> dict:
        cmd = request.get("command")
        if cmd == "status":
            return {"status": "ok", "data": {"state": "running"}}
        elif cmd == "poll":
            profile_id = request.get("profile_id")
            if not profile_id:
                return {"status": "error", "message": "profile_id required"}
            if self._poll_handler:
                asyncio.create_task(self._poll_handler(profile_id))
            return {"status": "ok", "data": {"profile_id": profile_id}}
        elif cmd == "shutdown":
            return {"status": "ok", "data": {"message": "shutting down"}}
        return {"status": "error", "message": f"unknown command: {cmd}"}
```

**Step 4: Run — expect 3 passed**

```bash
uv run pytest tests/test_socket_server.py -v
```

**Step 5: Commit**

```bash
git add src/scavenger/daemon/socket_server.py tests/test_socket_server.py
git commit -m "feat: add Unix domain socket server for daemon IPC"
```

---

### Task 12: Daemon main + scavenger-ctl CLI

**Files:**
- Create: `src/scavenger/daemon/main.py`
- Create: `src/scavenger/ctl.py`
- Create: `tests/test_ctl.py`

**Step 1: Write failing tests**

```python
# tests/test_ctl.py
from click.testing import CliRunner
from scavenger.ctl import cli


def test_help():
    result = CliRunner().invoke(cli, ["--help"])
    assert result.exit_code == 0

def test_list_profiles_missing_config(tmp_path):
    result = CliRunner().invoke(cli, ["--config", str(tmp_path / "nope.toml"), "list-profiles"])
    assert result.exit_code != 0 or "error" in result.output.lower() or "not found" in result.output.lower()

def test_list_profiles_with_config(tmp_path):
    cfg = tmp_path / "config.toml"
    cfg.write_text('[[profiles]]\nid = "sony"\nname = "Sony Glass"\nkeywords = ["sony"]\nnegative_keywords = []\nsources = ["ebay"]\nenabled = true\n')
    result = CliRunner().invoke(cli, ["--config", str(cfg), "list-profiles"])
    assert result.exit_code == 0
    assert "Sony Glass" in result.output
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_ctl.py -v
```

**Step 3: Implement `src/scavenger/daemon/main.py`**

```python
import asyncio
import logging
import signal
from datetime import datetime, timezone

from scavenger.config import AppConfig
from scavenger.daemon.scheduler import PollScheduler
from scavenger.daemon.socket_server import SocketServer
from scavenger.db import Database
from scavenger.models import Profile, Listing
from scavenger.plugins.ebay import EbayPlugin
from scavenger.plugins.craigslist import CraigslistPlugin
from scavenger.scoring import score_listing

logger = logging.getLogger(__name__)

BUNDLED_PLUGINS = {
    EbayPlugin.plugin_id: EbayPlugin(),
    CraigslistPlugin.plugin_id: CraigslistPlugin(),
}


class Daemon:
    def __init__(self, config: AppConfig):
        self._config = config
        self._db = Database(config.db_path)
        self._scheduler = PollScheduler()
        self._socket_server = SocketServer(config.socket_path)
        self._plugins = dict(BUNDLED_PLUGINS)

    def _register_profiles(self) -> None:
        for profile in self._config.profiles:
            if profile.enabled:
                self._scheduler.add_profile(profile, callback=self._make_callback(profile))

    def _make_callback(self, profile: Profile):
        async def callback(p: Profile) -> list[Listing]:
            return await self._poll_profile(p)
        return callback

    async def _poll_profile(self, profile: Profile) -> list[Listing]:
        new_listings = []
        for source_id in profile.sources:
            plugin = self._plugins.get(source_id)
            if plugin is None:
                logger.warning("Unknown plugin: %s", source_id)
                continue
            try:
                fetched = await plugin.fetch(profile)
                for listing in fetched:
                    listing.relevance_score = score_listing(
                        profile, listing.title, listing.description, listing.price
                    )
                    if listing.relevance_score == 0.0:
                        continue
                    if await self._db.upsert_listing(listing):
                        new_listings.append(listing)
                await self._db.update_source_state(source_id, last_polled=datetime.now(timezone.utc))
            except Exception:
                logger.exception("Poll failed for %s/%s", profile.id, source_id)
                await self._db.update_source_state(source_id, consecutive_errors=1)
        return new_listings

    async def run(self) -> None:
        await self._db.init()
        await self._scheduler.start()
        await self._socket_server.start()
        self._socket_server.register_poll_handler(self._handle_poll_command)
        self._register_profiles()
        logger.info("Daemon started")
        stop_event = asyncio.Event()
        loop = asyncio.get_running_loop()
        for sig in (signal.SIGINT, signal.SIGTERM):
            loop.add_signal_handler(sig, stop_event.set)
        await stop_event.wait()
        await self.shutdown()

    async def _handle_poll_command(self, profile_id: str) -> None:
        profile = next((p for p in self._config.profiles if p.id == profile_id), None)
        if profile:
            await self._poll_profile(profile)

    async def shutdown(self) -> None:
        await self._scheduler.stop()
        await self._socket_server.stop()
        await self._db.close()
```

**Step 4: Implement `src/scavenger/ctl.py`**

```python
import asyncio
import json
import socket
import sys
from pathlib import Path

import click

from scavenger.config import load_config, ConfigError

DEFAULT_CONFIG = Path("~/.config/scavenger/config.toml").expanduser()


def _send(socket_path: Path, command: dict) -> dict:
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(str(socket_path))
        sock.sendall(json.dumps(command).encode() + b"\n")
        data = b""
        while not data.endswith(b"\n"):
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk
        sock.close()
        return json.loads(data)
    except FileNotFoundError:
        return {"status": "error", "message": "Daemon not running"}
    except ConnectionRefusedError:
        return {"status": "error", "message": "Daemon not running"}


@click.group()
@click.option("--config", "config_path", default=str(DEFAULT_CONFIG), type=click.Path())
@click.pass_context
def cli(ctx, config_path):
    """scavenger-ctl — control the SCAVENGER daemon."""
    ctx.ensure_object(dict)
    ctx.obj["config_path"] = Path(config_path)


@cli.command("list-profiles")
@click.pass_context
def list_profiles(ctx):
    """List all configured interest profiles."""
    try:
        config = load_config(ctx.obj["config_path"])
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    for p in config.profiles:
        state = "enabled" if p.enabled else "disabled"
        click.echo(f"  [{state}] {p.name} ({p.id})")
        click.echo(f"           sources: {', '.join(p.sources)}  priority: {p.alert_priority}")


@cli.command()
@click.pass_context
def status(ctx):
    """Show daemon status."""
    try:
        config = load_config(ctx.obj["config_path"])
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    resp = _send(config.socket_path, {"command": "status"})
    click.echo("running" if resp["status"] == "ok" else f"error: {resp.get('message')}")


@cli.command()
@click.argument("profile_name")
@click.pass_context
def poll(ctx, profile_name):
    """Trigger immediate poll for a profile."""
    try:
        config = load_config(ctx.obj["config_path"])
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    matching = [p for p in config.profiles if p.name == profile_name or p.id == profile_name]
    if not matching:
        click.echo(f"Error: profile not found: {profile_name}", err=True)
        sys.exit(1)
    resp = _send(config.socket_path, {"command": "poll", "profile_id": matching[0].id})
    click.echo("ok" if resp["status"] == "ok" else f"error: {resp.get('message')}")


@cli.command()
@click.pass_context
def stop(ctx):
    """Stop the daemon."""
    try:
        config = load_config(ctx.obj["config_path"])
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    resp = _send(config.socket_path, {"command": "shutdown"})
    click.echo("ok" if resp["status"] == "ok" else f"error: {resp.get('message')}")


@cli.command()
@click.pass_context
def start(ctx):
    """Start the daemon."""
    try:
        config = load_config(ctx.obj["config_path"])
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    from scavenger.daemon.main import Daemon
    click.echo("Starting SCAVENGER daemon...")
    asyncio.run(Daemon(config).run())
```

**Step 5: Run — expect 3 passed**

```bash
uv run pytest tests/test_ctl.py -v
```

**Step 6: Run full suite**

```bash
uv run pytest -v
```
Expected: All tests passing, 0 failures.

**Step 7: Commit**

```bash
git add src/scavenger/daemon/main.py src/scavenger/ctl.py tests/test_ctl.py
git commit -m "feat: add daemon main entry point and scavenger-ctl CLI"
```

---

### Task 13: Final verification

**Step 1: Run full suite with coverage summary**

```bash
uv run pytest -v --tb=short
```
Expected: All tests passing.

**Step 2: Verify entry points**

```bash
uv run scavenger
uv run scavenger-ctl --help
uv run scavenger-ctl list-profiles --help
```

**Step 3: Commit if any loose files remain**

```bash
git status
git add -A
git commit -m "chore: M1 complete — all tests passing"
```
