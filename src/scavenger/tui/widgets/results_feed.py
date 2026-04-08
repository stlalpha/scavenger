import json
from datetime import datetime, timezone
from typing import Literal
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import ListItem, ListView, Label, Static
from textual.reactive import reactive
from scavenger.models import Listing
from scavenger.tui.messages import ListingSelected, ListingOpened

SRC_CLR = {"ebay": "#e6db74", "craigslist": "#f92672", "facebook": "#66d9ef"}

SortKey = Literal["newest", "oldest", "price_low", "price_high", "relevance", "source"]
SORT_LABELS: list[tuple[SortKey, str]] = [
    ("newest", "newest"),
    ("oldest", "oldest"),
    ("price_low", "price ↑"),
    ("price_high", "price ↓"),
    ("relevance", "score"),
    ("source", "source"),
]


def _sort_listings(listings: list[Listing], key: SortKey) -> list[Listing]:
    if key == "newest":
        return sorted(listings, key=lambda l: l.first_seen, reverse=True)
    if key == "oldest":
        return sorted(listings, key=lambda l: l.first_seen)
    if key == "price_low":
        return sorted(listings, key=lambda l: (l.price is None, l.price or 0))
    if key == "price_high":
        return sorted(listings, key=lambda l: (l.price is None, -(l.price or 0)))
    if key == "relevance":
        return sorted(listings, key=lambda l: l.relevance_score, reverse=True)
    if key == "source":
        return sorted(listings, key=lambda l: (l.source_id, l.first_seen), reverse=True)
    return listings


def _age(dt: datetime) -> str:
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    s = int((datetime.now(timezone.utc) - dt).total_seconds())
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


def _card(listing: Listing) -> str:
    icons = {
        "new": "[bold #66d9ef]●[/]",
        "seen": "[#444]·[/]",
        "saved": "[bold #a6e22e]★[/]",
        "dismissed": "[#333]✕[/]",
        "snoozed": "[#e6db74]◑[/]",
    }
    icon = icons.get(listing.status, " ")
    notable = " [bold #f92672]![/]" if _has_notable(listing) else ""
    if listing.status in ("seen", "dismissed"):
        title = f"[#777]{listing.title}[/]"
    else:
        title = f"[#f8f8f2]{listing.title}[/]"
    price = f"[bold #fd971f]${listing.price:,.0f}[/]" if listing.price else "[#444]--[/]"
    clr = SRC_CLR.get(listing.source_id, "#75715e")
    src = f"[{clr}]{listing.source_id[:2].upper()}[/]"
    age = f"[#555]{_age(listing.first_seen)}[/]"
    return f"{icon}{notable} {title}\n  {price}  {src}  {age}"


class ResultsFeed(Widget):
    BINDINGS = [
        ("j", "cursor_down", "Down"),
        ("k", "cursor_up", "Up"),
        ("down", "cursor_down", "Down"),
        ("up", "cursor_up", "Up"),
        ("enter", "open_listing", "Open"),
        ("S", "cycle_sort", "Sort"),
    ]

    DEFAULT_CSS = """
    ResultsFeed {
        width: 100%;
        height: 100%;
        background: #1a1a1a;
    }
    ResultsFeed #feed-hdr {
        dock: top;
        height: 1;
        padding: 0 1;
        background: #1a1a1a;
        color: #66d9ef;
        text-style: bold;
    }
    ResultsFeed ListView {
        height: 1fr;
        background: transparent;
        padding: 0;
    }
    ResultsFeed ListView > ListItem {
        padding: 0 1;
        height: auto;
        background: transparent;
        margin: 0 0 1 0;
    }
    ResultsFeed ListView > ListItem.--highlight {
        background: #252525;
    }
    """

    cursor: reactive[int] = reactive(0)

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._listings: list[Listing] = []
        self._raw_listings: list[Listing] = []
        self._listing_fingerprint: str = ""
        self._sort_key: SortKey = "newest"
        self._sort_idx: int = 0
        self._awaiting_poll: bool = True

    def compose(self) -> ComposeResult:
        yield Static(" LISTINGS", id="feed-hdr")
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
        self._listing_fingerprint = "__stale__"
        self._awaiting_poll = True

    async def update_listings(self, listings: list[Listing], from_poll: bool = False) -> None:
        if from_poll:
            self._awaiting_poll = False
        fp = "|".join(f"{l.id}:{l.status}" for l in listings)
        if fp == self._listing_fingerprint:
            return
        self._listing_fingerprint = fp
        self._raw_listings = listings
        sorted_listings = _sort_listings(listings, self._sort_key)
        self._listings = sorted_listings
        await self._render_list()

    async def _render_list(self) -> None:
        listings = self._listings
        lv = self.query_one(ListView)
        await lv.clear()
        for listing in listings:
            await lv.append(ListItem(Label(_card(listing))))
        self.cursor = min(self.cursor, max(0, len(listings) - 1))
        self._update_cursor()
        if not listings:
            if self._awaiting_poll:
                await lv.append(ListItem(Label("[#75715e italic]  waiting for results…[/]")))
            else:
                await lv.append(ListItem(Label("[#75715e italic]  no listings found[/]")))
        hdr = self.query_one("#feed-hdr", Static)
        sort_label = f"[#3a3a3a]{dict(SORT_LABELS)[self._sort_key]}[/]"
        nc = sum(1 for l in listings if l.status == "new")
        if nc > 0:
            hdr.update(f" LISTINGS [bold #f8f8f2]{nc} new[/] {sort_label}")
        elif listings:
            hdr.update(f" LISTINGS [#555]{len(listings)}[/] {sort_label}")
        elif self._awaiting_poll:
            hdr.update(" LISTINGS [#fd971f]polling...[/]")
        else:
            hdr.update(" LISTINGS [#555]empty[/]")

    def has_notable(self, listing_id: str) -> bool:
        return any(_has_notable(l) for l in self._listings if l.id == listing_id)

    def action_cursor_down(self) -> None:
        if self._listings:
            n = min(self.cursor + 1, len(self._listings) - 1)
            if n != self.cursor:
                self.cursor = n
                self._sync_list_view()

    def action_cursor_up(self) -> None:
        n = max(self.cursor - 1, 0)
        if n != self.cursor:
            self.cursor = n
            self._sync_list_view()

    def _sync_list_view(self) -> None:
        lv = self.query_one(ListView)
        if self._listings and 0 <= self.cursor < len(self._listings):
            lv.index = self.cursor

    def _update_cursor(self) -> None:
        self._sync_list_view()
        if self._listings and 0 <= self.cursor < len(self._listings):
            self.post_message(ListingSelected(listing=self._listings[self.cursor]))

    def on_list_view_highlighted(self, event: ListView.Highlighted) -> None:
        event.stop()
        lv = self.query_one(ListView)
        i = lv.index
        if i is not None and 0 <= i < len(self._listings):
            self.cursor = i
            self.post_message(ListingSelected(listing=self._listings[i]))

    def action_open_listing(self) -> None:
        if self._listings and 0 <= self.cursor < len(self._listings):
            self.post_message(ListingOpened(listing=self._listings[self.cursor]))

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        event.stop()
        lv = self.query_one(ListView)
        i = lv.index
        if i is not None and 0 <= i < len(self._listings):
            self.post_message(ListingOpened(listing=self._listings[i]))

    def action_cycle_sort(self) -> None:
        self._sort_idx = (self._sort_idx + 1) % len(SORT_LABELS)
        self._sort_key = SORT_LABELS[self._sort_idx][0]
        self._listings = _sort_listings(self._raw_listings, self._sort_key)
        self._listing_fingerprint = ""  # force re-render
        self.run_worker(self._render_list(), exclusive=True)

    def watch_cursor(self, cursor: int) -> None:
        self._update_cursor()
