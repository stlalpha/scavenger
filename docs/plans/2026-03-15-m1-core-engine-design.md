# M1: Core Engine Design

**Date:** 2026-03-15
**Status:** Approved
**Milestone:** M1 — Core Engine

---

## Overview

M1 establishes the foundation: config parsing, SQLite persistence, the daemon scheduler, the plugin interface, eBay and Craigslist source plugins, content-hash deduplication, and the `scavenger-ctl` CLI. No TUI in M1 — the daemon runs headlessly and `scavenger-ctl` provides control and basic output.

---

## Project Structure

```
scavenger/
├── pyproject.toml
├── src/
│   └── scavenger/
│       ├── __init__.py
│       ├── config.py          # TOML parsing + Pydantic profile validation
│       ├── models.py          # Listing, Profile, SourceState
│       ├── db.py              # aiosqlite schema + typed query helpers
│       ├── dedup.py           # content-hash deduplication
│       ├── daemon/
│       │   ├── __init__.py
│       │   ├── main.py        # asyncio entry point, scheduler, Unix socket server
│       │   └── scheduler.py   # APScheduler wrapper, per-profile job management
│       ├── plugins/
│       │   ├── base.py        # Plugin Protocol
│       │   ├── ebay.py        # eBay RSS + search
│       │   └── craigslist.py  # Craigslist RSS, per-city rotation
│       └── ctl.py             # scavenger-ctl CLI (Click)
└── tests/
    ├── test_config.py
    ├── test_dedup.py
    ├── test_db.py
    ├── test_plugins_ebay.py
    ├── test_plugins_craigslist.py
    └── test_scheduler.py
```

Two entry points in `pyproject.toml`:
- `scavenger` — placeholder for M2 TUI
- `scavenger-ctl` — daemon control and status CLI

User plugin directory: `~/.config/scavenger/plugins/` — any `.py` file exporting a class with `plugin_id` satisfying the `Plugin` Protocol is auto-discovered at daemon startup via `importlib`.

---

## Data Model

### Pydantic Models (`models.py`)

```python
class Listing(BaseModel):
    id: str                        # sha256(normalize_url(url)) — primary key
    profile_id: str
    source_id: str
    title: str
    description: str
    price: float | None
    currency: str = "USD"
    condition: str | None          # new | used | for_parts
    url: str
    image_urls: list[str]
    location: str | None
    first_seen: datetime
    last_seen: datetime
    relevance_score: float         # 0.0–100.0
    status: str = "new"            # new | seen | saved | dismissed | snoozed_until:<ts>

class Profile(BaseModel):
    id: str
    name: str
    keywords: list[str | list[str]]  # AND of ORs: [["sony", "minolta"], "a-mount"]
    negative_keywords: list[str]
    sources: list[str]
    price_min: float | None
    price_max: float | None
    poll_interval_sec: int = 900
    alert_priority: str = "normal"   # high | normal | low
    enabled: bool = True
    tags: list[str] = []
    escalation_keywords: list[str] = []
    location_radius_mi: int | None = None

class SourceState(BaseModel):
    plugin_id: str
    last_polled: datetime | None
    consecutive_errors: int = 0
    rate_limit_until: datetime | None = None
```

### SQLite Schema

WAL journal mode. Daemon owns all writes. TUI (M2+) reads only.

```sql
CREATE TABLE IF NOT EXISTS listings (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    price REAL,
    currency TEXT DEFAULT 'USD',
    condition TEXT,
    url TEXT NOT NULL,
    image_urls TEXT NOT NULL DEFAULT '[]',  -- JSON array
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
```

Schema initialized via `db.init_db()` at daemon startup — idempotent.

---

## Plugin Interface

```python
# plugins/base.py
class Plugin(Protocol):
    plugin_id: str

    async def fetch(self, profile: Profile) -> list[Listing]: ...
    async def supports_geo(self) -> bool: ...
```

### Bundled M1 Plugins

**`ebay.py`**
- Primary: eBay RSS search feed (`https://rss.ebay.com/rss2/...`)
- Secondary: eBay Finding API (unofficial) for price/condition filters
- Extracts: title, price, condition, image URL, listing URL, location

**`craigslist.py`**
- RSS feed per city per category
- Profile config supports list of city codes; plugin rotates through them
- Geo radius filtering applied post-fetch via ZIP→lat/lon lookup (offline dataset)
- Parses: title, price, location from RSS; fetches listing page for image URL

---

## Daemon Architecture

```
asyncio event loop (daemon/main.py)
├── APScheduler (AsyncIOScheduler)
│   └── one job per enabled profile
│       interval = profile.poll_interval_sec ± 10% jitter
│       job: plugin.fetch(profile) → dedup → score → db.upsert → notify
├── Unix socket server
│   path: ~/.run/scavenger/daemon.sock (XDG_RUNTIME_DIR fallback)
│   protocol: newline-delimited JSON commands
│   commands: status | poll <profile_id> | shutdown
└── Notification dispatcher
    new listing with status=new and alert_priority=high → libnotify / osascript
```

**Rate limiting:** Per-source minimum 30s between requests. Exponential backoff (base 2, max 1h) on consecutive errors. `sources` table tracks state across restarts.

---

## Deduplication

`dedup.py`:
1. Strip tracking params from URL (`utm_*`, `ref`, etc.)
2. `content_hash = sha256(normalized_url)`
3. Query `listings` by `id`:
   - Exists → update `last_seen`, insert `price_history` row if price changed, return `None` (no new alert)
   - Missing → return full `Listing` for insert with `status=new`

Cross-source deduplication deferred to M7 (requires fuzzy title matching).

---

## Relevance Scoring

Score 0–100 computed at fetch time per listing:

| Signal | Weight |
|--------|--------|
| Keyword match in title | 40 |
| Keyword match in description | 20 |
| Price within band | 20 |
| Recency (decay over 48h) | 10 |
| Condition match | 10 |

Keyword scoring: AND-of-ORs logic. Any negative keyword match → score = 0, listing discarded.

---

## `scavenger-ctl` CLI

```
scavenger-ctl start          # start daemon (systemd/launchd or foreground)
scavenger-ctl stop           # send shutdown command over socket
scavenger-ctl status         # show daemon status, per-source last-polled, error counts
scavenger-ctl list-profiles  # print all profiles from config with enabled state
scavenger-ctl poll <name>    # trigger immediate poll for named profile, stream results
scavenger-ctl install        # write systemd user unit or launchd plist and enable
```

---

## Configuration

`~/.config/scavenger/config.toml` — full schema per PRD §6.
`~/.config/scavenger/secrets.toml` — API keys, credentials (0600 permissions).
`~/.config/scavenger/plugins/` — user plugin directory.
`~/.local/share/scavenger/scavenger.db` — SQLite database (XDG data home).
`~/.cache/scavenger/images/` — image cache (M3+).

---

## Testing Strategy

- Unit tests for config parsing (valid/invalid TOML, profile validation)
- Unit tests for dedup (hash stability, URL normalization, price-change detection)
- Unit tests for relevance scorer
- Unit tests for eBay + Craigslist plugins using fixture RSS/HTML responses (no live network)
- Integration test for daemon scheduler: mock plugin, verify jobs fire at correct intervals
- Integration test for Unix socket: start daemon in subprocess, send commands, verify responses
