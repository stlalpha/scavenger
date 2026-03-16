import json
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import Static
from textual.containers import VerticalScroll
from scavenger.models import Listing


class DetailPanel(Widget):
    DEFAULT_CSS = """
    DetailPanel { width: 100%; height: 100%; padding: 1; }
    DetailPanel VerticalScroll { height: 100%; }
    """

    def __init__(self) -> None:
        super().__init__()
        self._listing: Listing | None = None

    def compose(self) -> ComposeResult:
        with VerticalScroll():
            yield Static("Select a listing", id="detail-content")

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
            f"{listing.title}\n{price} · {listing.source_id.upper()}\n{listing.url}"
            + (f"\n\n{ai_section.strip()}" if ai_section else "")
            + (f"\n\n{listing.description}" if listing.description else "")
            + "\n\n[o] open  [s] save  [d] dismiss  [n] snooze"
        )
        content.update(text)
