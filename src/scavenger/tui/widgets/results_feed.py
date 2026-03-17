import json
from datetime import datetime, timezone
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import ListItem, ListView, Label, Static
from textual.reactive import reactive
from scavenger.models import Listing
from scavenger.tui.messages import ListingSelected, ListingOpened

SOURCE_COLORS = {"ebay": "yellow", "craigslist": "magenta", "facebook": "blue"}
STATUS_ICONS = {
    "new": "[bold cyan]●[/]",
    "seen": "[dim]○[/]",
    "saved": "[bold green]★[/]",
    "dismissed": "[dim strike]✕[/]",
    "snoozed": "[dim yellow]◑[/]",
}


def _age(dt: datetime) -> str:
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    delta = datetime.now(timezone.utc) - dt
    s = int(delta.total_seconds())
    if s < 60:
        return "now"
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
    icon = STATUS_ICONS.get(listing.status, " ")
    notable = " [bold magenta]★[/]" if _has_notable(listing) else ""
    price = f"[bold]${listing.price:,.0f}[/]" if listing.price else "[dim]—[/]"
    src_color = SOURCE_COLORS.get(listing.source_id, "white")
    source = f"[{src_color}]{listing.source_id[:2].upper()}[/]"
    age = f"[dim]{_age(listing.first_seen)}[/]"
    title = listing.title
    return f"{icon}{notable} {title}\n   {price}  {source}  {age}"


class ResultsFeed(Widget):
    BINDINGS = [
        ("j", "cursor_down", "Down"),
        ("k", "cursor_up", "Up"),
        ("down", "cursor_down", "Down"),
        ("up", "cursor_up", "Up"),
        ("enter", "open_listing", "Open"),
    ]

    DEFAULT_CSS = """
    ResultsFeed {
        width: 100%;
        height: 100%;
        background: $surface;
    }
    ResultsFeed #feed-header {
        dock: top;
        height: 3;
        padding: 1 1 0 2;
        color: $text-muted;
        text-style: bold;
        background: $surface;
    }
    ResultsFeed ListView {
        height: 1fr;
        background: transparent;
    }
    ResultsFeed ListView > ListItem {
        padding: 0 1;
        height: auto;
    }
    ResultsFeed ListView > ListItem.--highlight {
        background: $boost;
    }
    """

    cursor: reactive[int] = reactive(0)

    def __init__(self) -> None:
        super().__init__()
        self._listings: list[Listing] = []
        self._listing_fingerprint: str = ""

    def compose(self) -> ComposeResult:
        yield Static("LISTINGS", id="feed-header")
        yield ListView()

    @property
    def listing_count(self) -> int:
        return len(self._listings)

    @property
    def focused_listing(self) -> Listing | None:
        if self._listings and 0 <= self.cursor < len(self._listings):
            return self._listings[self.cursor]
        return None

    def invalidate_fingerprint(self) -> None:
        self._listing_fingerprint = ""

    async def update_listings(self, listings: list[Listing]) -> None:
        fingerprint = "|".join(f"{l.id}:{l.status}" for l in listings)
        if fingerprint == self._listing_fingerprint:
            return
        self._listing_fingerprint = fingerprint
        self._listings = listings
        list_view = self.query_one(ListView)
        await list_view.clear()
        for listing in listings:
            await list_view.append(ListItem(Label(_card_label(listing))))
        self.cursor = min(self.cursor, max(0, len(listings) - 1))
        self._update_cursor()
        if not listings:
            await list_view.append(ListItem(Label("[dim]Waiting for results...[/]")))
        # Update header with count
        header = self.query_one("#feed-header", Static)
        new_count = sum(1 for l in listings if l.status == "new")
        if new_count > 0:
            header.update(f"LISTINGS [bold cyan]{new_count} new[/]")
        elif listings:
            header.update(f"LISTINGS [dim]{len(listings)}[/]")
        else:
            header.update("LISTINGS [dim yellow]polling...[/]")

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
        list_view = self.query_one(ListView)
        if self._listings and 0 <= self.cursor < len(self._listings):
            list_view.index = self.cursor

    def _update_cursor(self) -> None:
        self._sync_list_view()
        if self._listings and 0 <= self.cursor < len(self._listings):
            self.post_message(ListingSelected(listing=self._listings[self.cursor]))

    def on_list_view_highlighted(self, event: ListView.Highlighted) -> None:
        event.stop()
        list_view = self.query_one(ListView)
        i = list_view.index
        if i is not None and 0 <= i < len(self._listings):
            self.cursor = i
            self.post_message(ListingSelected(listing=self._listings[i]))

    def action_open_listing(self) -> None:
        if self._listings and 0 <= self.cursor < len(self._listings):
            self.post_message(ListingOpened(listing=self._listings[self.cursor]))

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        event.stop()
        list_view = self.query_one(ListView)
        i = list_view.index
        if i is not None and 0 <= i < len(self._listings):
            self.post_message(ListingOpened(listing=self._listings[i]))

    def watch_cursor(self, cursor: int) -> None:
        self._update_cursor()
