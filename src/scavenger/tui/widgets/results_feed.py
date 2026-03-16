import json
from datetime import datetime, timezone
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import ListItem, ListView, Label
from textual.reactive import reactive
from scavenger.models import Listing
from scavenger.tui.messages import ListingSelected


def _age(dt: datetime) -> str:
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    delta = datetime.now(timezone.utc) - dt
    s = int(delta.total_seconds())
    if s < 3600:
        return f"{s // 60}m"
    if s < 86400:
        return f"{s // 3600}h"
    return f"{s // 86400}d"


def _has_notable(listing: Listing) -> bool:
    if not listing.ai_evaluation:
        return False
    try:
        return bool(json.loads(listing.ai_evaluation).get("notable"))
    except Exception:
        return False


def _card_label(listing: Listing) -> str:
    star = "★ " if _has_notable(listing) else ""
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

    @property
    def focused_listing(self) -> Listing | None:
        if self._listings and 0 <= self.cursor < len(self._listings):
            return self._listings[self.cursor]
        return None

    async def update_listings(self, listings: list[Listing]) -> None:
        self._listings = listings
        list_view = self.query_one(ListView)
        await list_view.clear()
        for listing in listings:
            await list_view.append(ListItem(Label(_card_label(listing))))
        self.cursor = min(self.cursor, max(0, len(listings) - 1))
        self._update_cursor()

    def has_notable(self, listing_id: str) -> bool:
        return any(_has_notable(l) for l in self._listings if l.id == listing_id)

    def action_cursor_down(self) -> None:
        if self._listings:
            new_cursor = min(self.cursor + 1, len(self._listings) - 1)
            if new_cursor != self.cursor:
                self.cursor = new_cursor
                self._sync_list_view()

    def action_cursor_up(self) -> None:
        new_cursor = max(self.cursor - 1, 0)
        if new_cursor != self.cursor:
            self.cursor = new_cursor
            self._sync_list_view()

    def _sync_list_view(self) -> None:
        """Push cursor position into the ListView without re-triggering our handler."""
        list_view = self.query_one(ListView)
        if self._listings and 0 <= self.cursor < len(self._listings):
            list_view.index = self.cursor

    def _update_cursor(self) -> None:
        self._sync_list_view()
        if self._listings and 0 <= self.cursor < len(self._listings):
            self.post_message(ListingSelected(listing=self._listings[self.cursor]))

    def on_list_view_highlighted(self, event: ListView.Highlighted) -> None:
        """Sync cursor and show listing when ListView moves (keyboard or mouse)."""
        event.stop()
        list_view = self.query_one(ListView)
        i = list_view.index
        if i is not None and 0 <= i < len(self._listings):
            self.cursor = i
            self.post_message(ListingSelected(listing=self._listings[i]))

    def watch_cursor(self, cursor: int) -> None:
        self._update_cursor()
