import json
from datetime import datetime, timezone
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import ListItem, ListView, Label, Static
from textual.reactive import reactive
from scavenger.models import Listing
from scavenger.tui.messages import ListingSelected, ListingOpened

SRC_CLR = {"ebay": "#e6db74", "craigslist": "#f92672", "facebook": "#66d9ef"}


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
        "seen": "[#3a3a3a]·[/]",
        "saved": "[#a6e22e]★[/]",
        "dismissed": "[#3a3a3a]✕[/]",
        "snoozed": "[#e6db74]◑[/]",
    }
    icon = icons.get(listing.status, " ")
    notable = " [#f92672]★[/]" if _has_notable(listing) else ""
    price = f"[bold #fd971f]${listing.price:,.0f}[/]" if listing.price else "[#3a3a3a]—[/]"
    clr = SRC_CLR.get(listing.source_id, "#75715e")
    src = f"[{clr}]{listing.source_id[:2]}[/]"
    age = f"[#75715e]{_age(listing.first_seen)}[/]"
    return f"{icon}{notable} [#f8f8f2]{listing.title}[/]\n    {price} {src} {age}"


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
        background: #1a1a1a;
        border-left: solid #333;
    }
    ResultsFeed #feed-hdr {
        dock: top;
        height: 1;
        padding: 0 1;
        background: #252525;
        color: #75715e;
    }
    ResultsFeed ListView {
        height: 1fr;
        background: transparent;
        padding: 1 0;
    }
    ResultsFeed ListView > ListItem {
        padding: 0 1;
        height: auto;
        background: transparent;
    }
    ResultsFeed ListView > ListItem.--highlight {
        background: #2a2a2a;
    }
    """

    cursor: reactive[int] = reactive(0)

    def __init__(self) -> None:
        super().__init__()
        self._listings: list[Listing] = []
        self._listing_fingerprint: str = ""

    def compose(self) -> ComposeResult:
        yield Static("╶ listings", id="feed-hdr")
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
        fp = "|".join(f"{l.id}:{l.status}" for l in listings)
        if fp == self._listing_fingerprint:
            return
        self._listing_fingerprint = fp
        self._listings = listings
        lv = self.query_one(ListView)
        await lv.clear()
        for listing in listings:
            await lv.append(ListItem(Label(_card(listing))))
        self.cursor = min(self.cursor, max(0, len(listings) - 1))
        self._update_cursor()
        if not listings:
            await lv.append(ListItem(Label("[#75715e italic]  waiting for results…[/]")))
        hdr = self.query_one("#feed-hdr", Static)
        nc = sum(1 for l in listings if l.status == "new")
        if nc > 0:
            hdr.update(f"╶ listings [bold #66d9ef]{nc} new[/]")
        elif listings:
            hdr.update(f"╶ listings [#75715e]{len(listings)}[/]")
        else:
            hdr.update("╶ listings [#fd971f]polling…[/]")

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

    def watch_cursor(self, cursor: int) -> None:
        self._update_cursor()
