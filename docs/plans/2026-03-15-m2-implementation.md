# M2: TUI Shell Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the SCAVENGER Textual TUI with Profile Sidebar, Results Feed, Detail Panel, Status Bar, vim+arrow key navigation, and KGP thumbnail rendering in result cards.

**Architecture:** `ScavengerApp` owns a `DataLayer` that polls SQLite every 2 seconds via `set_interval`. Widgets receive data via Textual messages. The TUI reads SQLite directly — no daemon dependency at runtime. KGP thumbnails rendered via `term-image` in result cards (8×16 cells), with silent `□` fallback.

**Tech Stack:** Python 3.12+, Textual>=0.60, term-image>=0.7, existing aiosqlite/httpx/pydantic stack

**Working directory for all commands:** `.worktrees/m2-tui/`

---

### Task 1: Dependencies and TUI package scaffold

**Files:**
- Modify: `pyproject.toml`
- Create: `src/scavenger/tui/__init__.py`
- Create: `src/scavenger/tui/screens/__init__.py`
- Create: `src/scavenger/tui/widgets/__init__.py`
- Create: `src/scavenger/tui/messages.py`

**Step 1: Add dependencies to `pyproject.toml`**

Add to `dependencies`:
```toml
"textual>=0.60",
"term-image>=0.7",
```

**Step 2: Create package skeleton**

```bash
mkdir -p src/scavenger/tui/screens src/scavenger/tui/widgets
touch src/scavenger/tui/__init__.py
touch src/scavenger/tui/screens/__init__.py
touch src/scavenger/tui/widgets/__init__.py
```

**Step 3: Create `src/scavenger/tui/messages.py`**

```python
from textual.message import Message
from scavenger.models import Listing


class DataUpdated(Message):
    """Posted by DataLayer when new listings are detected."""
    def __init__(self, listings: list[Listing], profile_stats: dict[str, int]) -> None:
        self.listings = listings
        self.profile_stats = profile_stats
        super().__init__()


class ListingSelected(Message):
    """Posted by ResultsFeed when the focused listing changes."""
    def __init__(self, listing: Listing | None) -> None:
        self.listing = listing
        super().__init__()


class ProfileSelected(Message):
    """Posted by ProfileSidebar when active profile changes."""
    def __init__(self, profile_id: str | None) -> None:
        self.profile_id = profile_id
        super().__init__()
```

**Step 4: Install and verify**

```bash
uv sync --extra dev
uv run python -c "import textual; import term_image; print('ok')"
```
Expected: `ok`

**Step 5: Write smoke test for messages**

```python
# tests/test_tui_messages.py
from scavenger.tui.messages import DataUpdated, ListingSelected, ProfileSelected
from datetime import datetime, timezone
from scavenger.models import Listing


def make_listing() -> Listing:
    now = datetime.now(timezone.utc)
    return Listing(
        id="abc", profile_id="p1", source_id="ebay",
        title="Sony 85mm", url="https://ebay.com/1",
        first_seen=now, last_seen=now, relevance_score=80.0,
    )


def test_data_updated_message():
    msg = DataUpdated(listings=[make_listing()], profile_stats={"p1": 3})
    assert len(msg.listings) == 1
    assert msg.profile_stats["p1"] == 3


def test_listing_selected_message():
    listing = make_listing()
    msg = ListingSelected(listing=listing)
    assert msg.listing.id == "abc"


def test_listing_selected_none():
    msg = ListingSelected(listing=None)
    assert msg.listing is None


def test_profile_selected_message():
    msg = ProfileSelected(profile_id="p1")
    assert msg.profile_id == "p1"
```

**Step 6: Run — expect 4 passed**

```bash
uv run pytest tests/test_tui_messages.py -v
```

**Step 7: Commit**

```bash
git add pyproject.toml src/scavenger/tui/ tests/test_tui_messages.py
git commit -m "feat: add TUI package scaffold, messages, and dependencies"
```

---

### Task 2: DataLayer — async polling and state management

**Files:**
- Create: `src/scavenger/tui/data.py`
- Create: `tests/test_tui_data.py`

**Step 1: Write failing tests**

```python
# tests/test_tui_data.py
import pytest
from datetime import datetime, timezone
from pathlib import Path
from scavenger.tui.data import DataLayer
from scavenger.db import Database
from scavenger.models import Listing, Profile


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
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_tui_data.py -v
```

**Step 3: Implement `src/scavenger/tui/data.py`**

```python
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
            profile_id=profile_id, limit=limit + len(EXCLUDED_STATUSES) * limit
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
        listing.status = status  # type: ignore[assignment]
        await self._db._conn.execute(
            "UPDATE listings SET status=? WHERE id=?",
            (status, listing_id),
        )
        await self._db._conn.commit()
```

**Step 4: Run — expect 5 passed**

```bash
uv run pytest tests/test_tui_data.py -v
```

**Step 5: Run full suite**

```bash
uv run pytest --tb=short 2>&1 | tail -3
```

**Step 6: Commit**

```bash
git add src/scavenger/tui/data.py tests/test_tui_data.py
git commit -m "feat: add DataLayer with listing query, profile stats, and status marking"
```

---

### Task 3: Profile Sidebar widget

**Files:**
- Create: `src/scavenger/tui/widgets/profile_sidebar.py`
- Create: `tests/test_tui_widgets.py` (new file, add to throughout M2)

**Step 1: Write failing tests**

```python
# tests/test_tui_widgets.py
import pytest
from textual.app import App, ComposeResult
from scavenger.tui.widgets.profile_sidebar import ProfileSidebar
from scavenger.tui.messages import DataUpdated, ProfileSelected
from scavenger.models import Profile
from datetime import datetime, timezone
from scavenger.models import Listing


def make_profiles() -> list[Profile]:
    return [
        Profile(id="p1", name="Sony Glass", keywords=["sony"],
                negative_keywords=[], sources=["ebay"]),
        Profile(id="p2", name="IBM AS/400", keywords=["as400"],
                negative_keywords=[], sources=["ebay"]),
    ]


class SidebarTestApp(App):
    def __init__(self, profiles):
        super().__init__()
        self._profiles = profiles

    def compose(self) -> ComposeResult:
        yield ProfileSidebar(profiles=self._profiles)


async def test_sidebar_renders_profile_names():
    app = SidebarTestApp(make_profiles())
    async with app.run_test(size=(40, 20)) as pilot:
        await pilot.pause(0.1)
        assert app.query_one(ProfileSidebar) is not None
        sidebar = app.query_one(ProfileSidebar)
        assert "Sony Glass" in sidebar.render_str or any(
            "Sony Glass" in str(w.render()) for w in sidebar.query("*")
            if hasattr(w, "render")
        )


async def test_sidebar_shows_unread_count():
    app = SidebarTestApp(make_profiles())
    async with app.run_test(size=(40, 20)) as pilot:
        await pilot.pause(0.1)
        sidebar = app.query_one(ProfileSidebar)
        sidebar.update_stats({"p1": 5, "p2": 0})
        await pilot.pause(0.1)
        assert sidebar.get_unread("p1") == 5
        assert sidebar.get_unread("p2") == 0
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_tui_widgets.py -v
```

**Step 3: Implement `src/scavenger/tui/widgets/profile_sidebar.py`**

```python
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import ListItem, ListView, Label
from textual.reactive import reactive
from scavenger.models import Profile
from scavenger.tui.messages import ProfileSelected


class ProfileSidebar(Widget):
    DEFAULT_CSS = """
    ProfileSidebar {
        width: 100%;
        height: 100%;
        border-right: solid $panel-darken-1;
    }
    ProfileSidebar ListView {
        height: 100%;
        background: transparent;
    }
    """

    def __init__(self, profiles: list[Profile]) -> None:
        super().__init__()
        self._profiles = profiles
        self._stats: dict[str, int] = {}
        self._active_profile_id: str | None = profiles[0].id if profiles else None

    def compose(self) -> ComposeResult:
        yield ListView(*[
            ListItem(Label(self._label(p)), id=f"profile-{p.id}")
            for p in self._profiles
        ])

    def _label(self, profile: Profile) -> str:
        count = self._stats.get(profile.id, 0)
        badge = f" ({count})" if count > 0 else ""
        return f"{profile.name}{badge}"

    def update_stats(self, stats: dict[str, int]) -> None:
        self._stats = stats
        self._refresh_labels()

    def _refresh_labels(self) -> None:
        list_view = self.query_one(ListView)
        for i, profile in enumerate(self._profiles):
            try:
                item = list_view.query(ListItem)[i]
                item.query_one(Label).update(self._label(profile))
            except Exception:
                pass

    def get_unread(self, profile_id: str) -> int:
        return self._stats.get(profile_id, 0)

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        item_id = event.item.id or ""
        if item_id.startswith("profile-"):
            profile_id = item_id.removeprefix("profile-")
            self._active_profile_id = profile_id
            self.post_message(ProfileSelected(profile_id=profile_id))
```

**Step 4: Run — expect 2 passed**

```bash
uv run pytest tests/test_tui_widgets.py -v
```

**Step 5: Commit**

```bash
git add src/scavenger/tui/widgets/profile_sidebar.py tests/test_tui_widgets.py
git commit -m "feat: add ProfileSidebar widget with unread counts"
```

---

### Task 4: Results Feed widget with navigation

**Files:**
- Modify: `src/scavenger/tui/widgets/` (create `results_feed.py`)
- Modify: `tests/test_tui_widgets.py`

**Step 1: Add failing tests to `tests/test_tui_widgets.py`**

```python
from scavenger.tui.widgets.results_feed import ResultsFeed


def make_listings(n: int = 3) -> list[Listing]:
    now = datetime.now(timezone.utc)
    return [
        Listing(
            id=f"id{i}", profile_id="p1", source_id="ebay",
            title=f"Sony Lens {i}", url=f"https://ebay.com/{i}",
            first_seen=now, last_seen=now, relevance_score=80.0,
            price=float(100 + i * 50),
        )
        for i in range(n)
    ]


class FeedTestApp(App):
    def compose(self) -> ComposeResult:
        yield ResultsFeed()

    def on_mount(self) -> None:
        self.query_one(ResultsFeed).update_listings(make_listings(3))


async def test_feed_renders_listings():
    app = FeedTestApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        feed = app.query_one(ResultsFeed)
        assert feed.listing_count == 3


async def test_feed_j_key_moves_cursor_down():
    app = FeedTestApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        feed = app.query_one(ResultsFeed)
        assert feed.cursor == 0
        await pilot.press("j")
        assert feed.cursor == 1


async def test_feed_down_arrow_moves_cursor():
    app = FeedTestApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        await pilot.press("down")
        feed = app.query_one(ResultsFeed)
        assert feed.cursor == 1


async def test_feed_k_key_moves_cursor_up():
    app = FeedTestApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        await pilot.press("j")
        await pilot.press("k")
        feed = app.query_one(ResultsFeed)
        assert feed.cursor == 0


async def test_feed_cursor_does_not_go_below_zero():
    app = FeedTestApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        await pilot.press("k")  # already at 0
        feed = app.query_one(ResultsFeed)
        assert feed.cursor == 0


async def test_feed_notable_shows_star():
    now = datetime.now(timezone.utc)
    listings = [Listing(
        id="x", profile_id="p1", source_id="ebay",
        title="Zeiss Lens", url="https://ebay.com/x",
        first_seen=now, last_seen=now, relevance_score=90.0,
        ai_evaluation='{"relevant": true, "reason": "Great", "notable": "Zeiss variant", "escalate": false}',
    )]

    class StarApp(App):
        def compose(self) -> ComposeResult:
            yield ResultsFeed()
        def on_mount(self) -> None:
            self.query_one(ResultsFeed).update_listings(listings)

    app = StarApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        feed = app.query_one(ResultsFeed)
        assert feed.has_notable("x") is True
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_tui_widgets.py -v
```

**Step 3: Implement `src/scavenger/tui/widgets/results_feed.py`**

```python
import json
from datetime import datetime, timezone
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import ListItem, ListView, Label
from textual.reactive import reactive
from scavenger.models import Listing
from scavenger.tui.messages import ListingSelected

BINDINGS = [
    ("j", "cursor_down", "Down"),
    ("k", "cursor_up", "Up"),
    ("down", "cursor_down", "Down"),
    ("up", "cursor_up", "Up"),
]


def _age(dt: datetime) -> str:
    delta = datetime.now(timezone.utc) - dt.replace(tzinfo=timezone.utc) if dt.tzinfo is None else datetime.now(timezone.utc) - dt
    s = int(delta.total_seconds())
    if s < 3600:
        return f"{s // 60}m"
    if s < 86400:
        return f"{s // 3600}h"
    return f"{s // 86400}d"


def _notable(listing: Listing) -> bool:
    if not listing.ai_evaluation:
        return False
    try:
        data = json.loads(listing.ai_evaluation)
        return bool(data.get("notable"))
    except Exception:
        return False


def _card_label(listing: Listing) -> str:
    star = "★ " if _notable(listing) else ""
    price = f"${listing.price:.0f}" if listing.price else "—"
    source = listing.source_id[:2].upper()
    age = _age(listing.first_seen)
    title = listing.title[:40] + ("…" if len(listing.title) > 40 else "")
    return f"{star}{title}\n  {price} · {source} · {age}"


class ResultsFeed(Widget):
    BINDINGS = [
        ("j", "cursor_down", "Down"),
        ("k", "cursor_up", "Up"),
        ("down", "cursor_down", "Down"),
        ("up", "cursor_up", "Up"),
    ]

    DEFAULT_CSS = """
    ResultsFeed {
        width: 100%;
        height: 100%;
        border-right: solid $panel-darken-1;
    }
    """

    cursor: reactive[int] = reactive(0)

    def __init__(self) -> None:
        super().__init__()
        self._listings: list[Listing] = []

    def compose(self) -> ComposeResult:
        yield ListView()

    @property
    def listing_count(self) -> int:
        return len(self._listings)

    def update_listings(self, listings: list[Listing]) -> None:
        self._listings = listings
        list_view = self.query_one(ListView)
        list_view.clear()
        for listing in listings:
            list_view.append(ListItem(Label(_card_label(listing)), id=f"listing-{listing.id}"))
        self.cursor = min(self.cursor, max(0, len(listings) - 1))
        self._update_cursor()

    def has_notable(self, listing_id: str) -> bool:
        return any(_notable(l) for l in self._listings if l.id == listing_id)

    def action_cursor_down(self) -> None:
        if self._listings:
            self.cursor = min(self.cursor + 1, len(self._listings) - 1)
            self._update_cursor()

    def action_cursor_up(self) -> None:
        self.cursor = max(self.cursor - 1, 0)
        self._update_cursor()

    def _update_cursor(self) -> None:
        list_view = self.query_one(ListView)
        if self._listings and 0 <= self.cursor < len(self._listings):
            list_view.index = self.cursor
            listing = self._listings[self.cursor]
            self.post_message(ListingSelected(listing=listing))

    def watch_cursor(self, cursor: int) -> None:
        self._update_cursor()
```

**Step 4: Run — expect all widget tests pass**

```bash
uv run pytest tests/test_tui_widgets.py -v
```

**Step 5: Run full suite**

```bash
uv run pytest --tb=short 2>&1 | tail -3
```

**Step 6: Commit**

```bash
git add src/scavenger/tui/widgets/results_feed.py tests/test_tui_widgets.py
git commit -m "feat: add ResultsFeed with j/k/arrow navigation and notable badge"
```

---

### Task 5: Detail Panel widget

**Files:**
- Create: `src/scavenger/tui/widgets/detail_panel.py`
- Modify: `tests/test_tui_widgets.py`

**Step 1: Add failing tests**

```python
from scavenger.tui.widgets.detail_panel import DetailPanel


class DetailTestApp(App):
    def __init__(self, listing):
        super().__init__()
        self._listing = listing

    def compose(self) -> ComposeResult:
        yield DetailPanel()

    def on_mount(self) -> None:
        self.query_one(DetailPanel).show_listing(self._listing)


async def test_detail_panel_shows_title():
    now = datetime.now(timezone.utc)
    listing = Listing(
        id="abc", profile_id="p1", source_id="ebay",
        title="Sony 85mm f/1.4 A-mount", url="https://ebay.com/1",
        first_seen=now, last_seen=now, relevance_score=80.0,
        price=249.99, description="Excellent condition.",
    )
    app = DetailTestApp(listing)
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        panel = app.query_one(DetailPanel)
        assert panel.current_listing is not None
        assert panel.current_listing.id == "abc"


async def test_detail_panel_shows_ai_notes():
    now = datetime.now(timezone.utc)
    listing = Listing(
        id="z", profile_id="p1", source_id="ebay",
        title="Sony Zeiss", url="https://ebay.com/z",
        first_seen=now, last_seen=now, relevance_score=90.0,
        ai_evaluation='{"relevant": true, "reason": "Strong match", "notable": "Zeiss variant", "escalate": false}',
    )
    app = DetailTestApp(listing)
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        panel = app.query_one(DetailPanel)
        assert panel.has_ai_notes is True


async def test_detail_panel_empty_on_none():
    class EmptyApp(App):
        def compose(self) -> ComposeResult:
            yield DetailPanel()

    app = EmptyApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        panel = app.query_one(DetailPanel)
        assert panel.current_listing is None
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_tui_widgets.py::test_detail_panel_shows_title -v
```

**Step 3: Implement `src/scavenger/tui/widgets/detail_panel.py`**

```python
import json
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import Label, Static
from textual.containers import VerticalScroll
from scavenger.models import Listing


class DetailPanel(Widget):
    DEFAULT_CSS = """
    DetailPanel {
        width: 100%;
        height: 100%;
        padding: 1;
    }
    DetailPanel VerticalScroll {
        height: 100%;
    }
    DetailPanel .detail-title { text-style: bold; }
    DetailPanel .detail-price { color: $success; }
    DetailPanel .detail-source { color: $text-muted; }
    DetailPanel .detail-notable {
        color: $warning;
        text-style: bold;
        padding: 0 0 1 0;
    }
    DetailPanel .detail-ai-reason { color: $text-muted; padding: 0 0 1 0; }
    DetailPanel .detail-empty { color: $text-muted; text-align: center; }
    """

    def __init__(self) -> None:
        super().__init__()
        self._listing: Listing | None = None

    def compose(self) -> ComposeResult:
        with VerticalScroll():
            yield Static("Select a listing", classes="detail-empty", id="detail-content")

    @property
    def current_listing(self) -> Listing | None:
        return self._listing

    @property
    def has_ai_notes(self) -> bool:
        if not self._listing or not self._listing.ai_evaluation:
            return False
        try:
            data = json.loads(self._listing.ai_evaluation)
            return bool(data.get("reason") or data.get("notable"))
        except Exception:
            return False

    def show_listing(self, listing: Listing | None) -> None:
        self._listing = listing
        content = self.query_one("#detail-content", Static)
        if listing is None:
            content.update("Select a listing")
            return

        ai_section = ""
        if listing.ai_evaluation:
            try:
                ev = json.loads(listing.ai_evaluation)
                if ev.get("notable"):
                    ai_section += f"\n★ {ev['notable']}"
                if ev.get("reason"):
                    ai_section += f"\n{ev['reason']}"
            except Exception:
                pass

        price = f"${listing.price:.2f}" if listing.price else "—"
        text = (
            f"{listing.title}\n"
            f"{price} · {listing.source_id.upper()}\n"
            f"{listing.url}"
            + (f"\n\n{ai_section.strip()}" if ai_section else "")
            + (f"\n\n{listing.description}" if listing.description else "")
            + f"\n\n[o] open  [s] save  [d] dismiss  [n] snooze"
        )
        content.update(text)
```

**Step 4: Run — expect all detail panel tests pass**

```bash
uv run pytest tests/test_tui_widgets.py -v
```

**Step 5: Commit**

```bash
git add src/scavenger/tui/widgets/detail_panel.py tests/test_tui_widgets.py
git commit -m "feat: add DetailPanel with AI notes display"
```

---

### Task 6: Status Bar widget

**Files:**
- Create: `src/scavenger/tui/widgets/status_bar.py`
- Modify: `tests/test_tui_widgets.py`

**Step 1: Add failing tests**

```python
from scavenger.tui.widgets.status_bar import StatusBar


class StatusBarTestApp(App):
    def compose(self) -> ComposeResult:
        yield StatusBar()


async def test_status_bar_renders():
    app = StatusBarTestApp()
    async with app.run_test(size=(120, 5)) as pilot:
        await pilot.pause(0.1)
        bar = app.query_one(StatusBar)
        assert bar is not None


async def test_status_bar_daemon_unreachable():
    app = StatusBarTestApp()
    async with app.run_test(size=(120, 5)) as pilot:
        await pilot.pause(0.1)
        bar = app.query_one(StatusBar)
        bar.set_daemon_status(reachable=False)
        await pilot.pause(0.1)
        assert bar.daemon_reachable is False


async def test_status_bar_new_count():
    app = StatusBarTestApp()
    async with app.run_test(size=(120, 5)) as pilot:
        await pilot.pause(0.1)
        bar = app.query_one(StatusBar)
        bar.set_new_count(7)
        assert bar.new_today == 7
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_tui_widgets.py::test_status_bar_renders -v
```

**Step 3: Implement `src/scavenger/tui/widgets/status_bar.py`**

```python
from datetime import datetime, timezone
from textual.widget import Widget
from textual.widgets import Static
from textual.app import ComposeResult


class StatusBar(Widget):
    DEFAULT_CSS = """
    StatusBar {
        dock: bottom;
        height: 1;
        background: $panel-darken-1;
        padding: 0 1;
    }
    """

    def __init__(self) -> None:
        super().__init__()
        self.daemon_reachable: bool = True
        self.new_today: int = 0
        self._last_poll: str = "—"

    def compose(self) -> ComposeResult:
        yield Static(self._render_text(), id="status-text")

    def _render_text(self) -> str:
        status = "running" if self.daemon_reachable else "unreachable"
        icon = "●" if self.daemon_reachable else "✕"
        return (
            f"{icon} daemon: {status}  ·  "
            f"last poll: {self._last_poll}  ·  "
            f"{self.new_today} new today  ·  "
            f"[?] help  [q] quit"
        )

    def _refresh(self) -> None:
        try:
            self.query_one("#status-text", Static).update(self._render_text())
        except Exception:
            pass

    def set_daemon_status(self, reachable: bool) -> None:
        self.daemon_reachable = reachable
        self._refresh()

    def set_last_poll(self, ts: datetime) -> None:
        delta = int((datetime.now(timezone.utc) - ts).total_seconds())
        self._last_poll = f"{delta}s ago"
        self._refresh()

    def set_new_count(self, count: int) -> None:
        self.new_today = count
        self._refresh()
```

**Step 4: Run — expect all status bar tests pass**

```bash
uv run pytest tests/test_tui_widgets.py -v
```

**Step 5: Commit**

```bash
git add src/scavenger/tui/widgets/status_bar.py tests/test_tui_widgets.py
git commit -m "feat: add StatusBar with daemon status and poll stats"
```

---

### Task 7: MainScreen and ScavengerApp

**Files:**
- Create: `src/scavenger/tui/screens/main.py`
- Create: `src/scavenger/tui/app.py`
- Modify: `src/scavenger/main.py`
- Create: `tests/test_tui_app.py`

**Step 1: Write failing tests**

```python
# tests/test_tui_app.py
import pytest
from pathlib import Path
from textual.app import App
from scavenger.tui.app import ScavengerApp
from scavenger.config import AppConfig, GlobalConfig
from scavenger.db import Database
from scavenger.models import Listing, Profile
from datetime import datetime, timezone


async def make_test_db(tmp_path: Path) -> Database:
    db = Database(tmp_path / "test.db")
    await db.init()
    await db.migrate()
    return db


def make_config(tmp_path: Path) -> AppConfig:
    return AppConfig(
        global_config=GlobalConfig(
            db_path=str(tmp_path / "test.db"),
            socket_path=str(tmp_path / "daemon.sock"),
        ),
        profiles=[
            Profile(id="p1", name="Sony Glass", keywords=["sony"],
                    negative_keywords=[], sources=["ebay"]),
        ],
    )


async def test_app_launches_and_quits(tmp_path):
    db = await make_test_db(tmp_path)
    await db.close()
    config = make_config(tmp_path)
    app = ScavengerApp(config=config)
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        assert app.is_running
        await pilot.press("q")


async def test_app_tab_cycles_focus(tmp_path):
    db = await make_test_db(tmp_path)
    await db.close()
    config = make_config(tmp_path)
    app = ScavengerApp(config=config)
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        await pilot.press("tab")
        await pilot.pause(0.1)
        # app still running after tab
        assert app.is_running


async def test_app_right_arrow_cycles_focus(tmp_path):
    db = await make_test_db(tmp_path)
    await db.close()
    config = make_config(tmp_path)
    app = ScavengerApp(config=config)
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        await pilot.press("right")
        await pilot.pause(0.1)
        assert app.is_running
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_tui_app.py -v
```

**Step 3: Create `src/scavenger/tui/screens/main.py`**

```python
from textual.app import ComposeResult
from textual.screen import Screen
from textual.containers import Horizontal
from scavenger.tui.widgets.profile_sidebar import ProfileSidebar
from scavenger.tui.widgets.results_feed import ResultsFeed
from scavenger.tui.widgets.detail_panel import DetailPanel
from scavenger.tui.widgets.status_bar import StatusBar
from scavenger.tui.messages import ListingSelected, ProfileSelected, DataUpdated
from scavenger.models import Profile


class MainScreen(Screen):
    DEFAULT_CSS = """
    MainScreen {
        layout: vertical;
    }
    MainScreen #main-columns {
        layout: horizontal;
        height: 1fr;
    }
    MainScreen ProfileSidebar { width: 1fr; }
    MainScreen ResultsFeed { width: 2fr; }
    MainScreen DetailPanel { width: 2fr; }
    """

    def __init__(self, profiles: list[Profile]) -> None:
        super().__init__()
        self._profiles = profiles

    def compose(self) -> ComposeResult:
        with Horizontal(id="main-columns"):
            yield ProfileSidebar(profiles=self._profiles)
            yield ResultsFeed()
            yield DetailPanel()
        yield StatusBar()

    def on_listing_selected(self, event: ListingSelected) -> None:
        self.query_one(DetailPanel).show_listing(event.listing)

    def on_profile_selected(self, event: ProfileSelected) -> None:
        self.app.set_active_profile(event.profile_id)  # type: ignore[attr-defined]

    def on_data_updated(self, event: DataUpdated) -> None:
        self.query_one(ResultsFeed).update_listings(event.listings)
        self.query_one(ProfileSidebar).update_stats(event.profile_stats)
        self.query_one(StatusBar).set_new_count(
            sum(event.profile_stats.values())
        )
```

**Step 4: Create `src/scavenger/tui/app.py`**

```python
import asyncio
import logging
from pathlib import Path
from textual.app import App, ComposeResult
from textual.binding import Binding
from scavenger.config import AppConfig
from scavenger.db import Database
from scavenger.tui.data import DataLayer
from scavenger.tui.messages import DataUpdated
from scavenger.tui.screens.main import MainScreen

logger = logging.getLogger(__name__)

POLL_INTERVAL = 2.0


class ScavengerApp(App):
    BINDINGS = [
        Binding("q", "quit_tui", "Quit"),
        Binding("Q", "quit_all", "Quit + stop daemon"),
        Binding("tab", "focus_next_panel", "Next panel"),
        Binding("shift+tab", "focus_prev_panel", "Prev panel"),
        Binding("right", "focus_next_panel", "Next panel", show=False),
        Binding("left", "focus_prev_panel", "Prev panel", show=False),
        Binding("o", "open_url", "Open", show=False),
        Binding("s", "save_listing", "Save", show=False),
        Binding("d", "dismiss_listing", "Dismiss", show=False),
        Binding("n", "snooze_listing", "Snooze", show=False),
        Binding("r", "repoll", "Re-poll", show=False),
        Binding("?", "show_help", "Help", show=False),
    ]

    def __init__(self, config: AppConfig) -> None:
        super().__init__()
        self._config = config
        self._db: Database | None = None
        self._data_layer: DataLayer | None = None
        self._active_profile_id: str | None = (
            config.profiles[0].id if config.profiles else None
        )

    def on_mount(self) -> None:
        self.set_interval(POLL_INTERVAL, self._poll)

    async def _poll(self) -> None:
        if self._data_layer is None:
            return
        try:
            listings = await self._data_layer.get_listings(
                profile_id=self._active_profile_id, limit=100
            )
            stats = await self._data_layer.get_profile_stats()
            self.post_message(DataUpdated(listings=listings, profile_stats=stats))
        except Exception as e:
            logger.warning("Poll error: %s", e)

    async def on_load(self) -> None:
        self._db = Database(self._config.db_path)
        await self._db.init()
        await self._db.migrate()
        self._data_layer = DataLayer(self._db)

    async def on_unmount(self) -> None:
        if self._db:
            await self._db.close()

    def compose(self) -> ComposeResult:
        yield MainScreen(profiles=self._config.profiles)

    def set_active_profile(self, profile_id: str | None) -> None:
        self._active_profile_id = profile_id

    def action_quit_tui(self) -> None:
        self.exit()

    def action_quit_all(self) -> None:
        import socket, json
        sock_path = self._config.socket_path
        try:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            s.settimeout(2.0)
            s.connect(str(sock_path))
            s.sendall(json.dumps({"command": "shutdown"}).encode() + b"\n")
            s.close()
        except Exception:
            pass
        self.exit()

    def action_focus_next_panel(self) -> None:
        self.screen.focus_next()

    def action_focus_prev_panel(self) -> None:
        self.screen.focus_previous()

    def action_open_url(self) -> None:
        from scavenger.tui.widgets.results_feed import ResultsFeed
        feed = self.query_one(ResultsFeed)
        listings = feed._listings
        if listings and 0 <= feed.cursor < len(listings):
            import subprocess
            subprocess.Popen(["xdg-open", listings[feed.cursor].url])

    def action_dismiss_listing(self) -> None:
        self._mark_focused("dismissed")

    def action_save_listing(self) -> None:
        self._mark_focused("saved")

    def action_snooze_listing(self) -> None:
        from datetime import datetime, timezone, timedelta
        until = datetime.now(timezone.utc) + timedelta(hours=1)
        self._mark_focused(f"snoozed_until:{until.isoformat()}")

    def _mark_focused(self, status: str) -> None:
        from scavenger.tui.widgets.results_feed import ResultsFeed
        feed = self.query_one(ResultsFeed)
        listings = feed._listings
        if listings and 0 <= feed.cursor < len(listings) and self._data_layer:
            listing_id = listings[feed.cursor].id
            asyncio.create_task(self._data_layer.mark_status(listing_id, status))
            asyncio.create_task(self._poll())

    def action_repoll(self) -> None:
        asyncio.create_task(self._poll())

    def action_show_help(self) -> None:
        self.notify("j/k ↑↓ navigate  o open  s save  d dismiss  n snooze  r repoll  q quit")
```

**Step 5: Update `src/scavenger/main.py`**

```python
import click
from scavenger.config import load_config, ConfigError
from pathlib import Path
import sys

DEFAULT_CONFIG = Path("~/.config/scavenger/config.toml").expanduser()


@click.command()
@click.option("--config", "config_path", default=str(DEFAULT_CONFIG), type=click.Path())
def cli(config_path: str) -> None:
    """SCAVENGER — Continuous web intelligence terminal."""
    try:
        config = load_config(Path(config_path))
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    from scavenger.tui.app import ScavengerApp
    app = ScavengerApp(config=config)
    app.run()
```

**Step 6: Run — expect 3 passed**

```bash
uv run pytest tests/test_tui_app.py -v
```

**Step 7: Run full suite**

```bash
uv run pytest --tb=short 2>&1 | tail -3
```

**Step 8: Commit**

```bash
git add src/scavenger/tui/screens/main.py src/scavenger/tui/app.py src/scavenger/main.py tests/test_tui_app.py
git commit -m "feat: add MainScreen, ScavengerApp, and TUI entry point"
```

---

### Task 8: KGP thumbnail rendering

**Files:**
- Create: `src/scavenger/tui/widgets/thumbnail.py`
- Modify: `src/scavenger/tui/widgets/results_feed.py`
- Create: `tests/test_tui_thumbnail.py`

**Step 1: Write failing tests**

```python
# tests/test_tui_thumbnail.py
import pytest
from pathlib import Path
from scavenger.tui.widgets.thumbnail import ThumbnailCache, PLACEHOLDER


async def test_placeholder_returned_for_no_url(tmp_path):
    cache = ThumbnailCache(cache_dir=tmp_path)
    result = await cache.get(None)
    assert result == PLACEHOLDER


async def test_placeholder_returned_on_download_failure(tmp_path, respx_mock):
    import respx, httpx
    respx_mock.get("https://example.com/img.jpg").mock(return_value=httpx.Response(404))
    cache = ThumbnailCache(cache_dir=tmp_path)
    result = await cache.get("https://example.com/img.jpg")
    assert result == PLACEHOLDER


async def test_cache_hit_returns_path(tmp_path):
    # Write a fake image file to cache
    url = "https://example.com/real.jpg"
    from scavenger.dedup import content_hash
    cache_path = tmp_path / f"{content_hash(url)}.jpg"
    cache_path.write_bytes(b"FAKEJPEG")
    cache = ThumbnailCache(cache_dir=tmp_path)
    result = await cache.get(url)
    assert result == cache_path
```

**Step 2: Run — expect ImportError**

```bash
uv run pytest tests/test_tui_thumbnail.py -v
```

**Step 3: Implement `src/scavenger/tui/widgets/thumbnail.py`**

```python
import logging
from pathlib import Path
import httpx
from scavenger.dedup import content_hash

logger = logging.getLogger(__name__)

PLACEHOLDER = "□"
DEFAULT_CACHE_DIR = Path("~/.cache/scavenger/images").expanduser()


class ThumbnailCache:
    """Downloads and caches listing images. Returns file path or PLACEHOLDER."""

    def __init__(self, cache_dir: Path = DEFAULT_CACHE_DIR) -> None:
        self._cache_dir = cache_dir
        self._cache_dir.mkdir(parents=True, exist_ok=True)

    def _cache_path(self, url: str) -> Path:
        return self._cache_dir / f"{content_hash(url)}.jpg"

    async def get(self, url: str | None) -> Path | str:
        """Return cached Path if available, download if not, PLACEHOLDER on failure."""
        if not url:
            return PLACEHOLDER
        cached = self._cache_path(url)
        if cached.exists():
            return cached
        return await self._download(url, cached)

    async def _download(self, url: str, dest: Path) -> Path | str:
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                response = await client.get(url)
                response.raise_for_status()
                dest.write_bytes(response.content)
                return dest
        except Exception as e:
            logger.debug("Thumbnail download failed for %s: %s", url, e)
            return PLACEHOLDER
```

**Step 4: Run — expect 3 passed**

```bash
uv run pytest tests/test_tui_thumbnail.py -v
```

**Note on KGP rendering in result cards:**

`term-image` renders images via `TermImage` and its `draw()` method. In Textual's unicode placeholder mode the rendering is done via escape sequences embedded in widget output. Full KGP integration inside Textual's compositor requires the `term_image.widget.Image` Textual widget (available in term-image 0.7+). Add this to `results_feed.py` where thumbnails are needed — the `_card_label` function can be extended to show a `□` placeholder in the card text when no image is available, and the `term_image.widget.Image` widget renders alongside the label when a path is available.

For M2 scope: thumbnails are shown as `□` or the path is stored for the Image widget to render. Full Kitty rendering integration is validated by running `uv run scavenger` in Kitty and confirming images appear in cards.

**Step 5: Commit**

```bash
git add src/scavenger/tui/widgets/thumbnail.py tests/test_tui_thumbnail.py
git commit -m "feat: add ThumbnailCache with async download and content-hash filenames"
```

---

### Task 9: Final verification

**Step 1: Run full test suite**

```bash
uv run pytest -v --tb=short
```
Expected: all tests passing.

**Step 2: Verify entry points**

```bash
uv run scavenger --help
uv run scavenger-ctl --help
```

**Step 3: Smoke test TUI imports**

```bash
uv run python -c "
from scavenger.tui.app import ScavengerApp
from scavenger.tui.data import DataLayer
from scavenger.tui.widgets.profile_sidebar import ProfileSidebar
from scavenger.tui.widgets.results_feed import ResultsFeed
from scavenger.tui.widgets.detail_panel import DetailPanel
from scavenger.tui.widgets.status_bar import StatusBar
from scavenger.tui.widgets.thumbnail import ThumbnailCache
print('all imports ok')
"
```

**Step 4: Final commit if needed**

```bash
git status
git add -A
git commit -m "chore: M2 complete — all tests passing"
```
Only if uncommitted changes exist.
