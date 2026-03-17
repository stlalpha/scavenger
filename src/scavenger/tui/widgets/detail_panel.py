import json
import logging
from pathlib import Path
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import Static
from textual.containers import VerticalScroll
from scavenger.models import Listing
from scavenger.tui.widgets.thumbnail import ThumbnailCache

logger = logging.getLogger(__name__)

try:
    import os
    if os.environ.get("TERM", "") == "xterm-kitty":
        from textual_image.widget import TGPImage as KittyImage
    else:
        from textual_image.widget import Image as KittyImage
    HAS_IMAGE_WIDGET = True
except ImportError:
    HAS_IMAGE_WIDGET = False

SOURCE_COLORS = {"ebay": "yellow", "craigslist": "magenta", "facebook": "blue"}
STATUS_LABELS = {
    "new": "[bold cyan]NEW[/]",
    "seen": "[dim]SEEN[/]",
    "saved": "[bold green]SAVED[/]",
    "dismissed": "[dim]DISMISSED[/]",
    "snoozed": "[yellow]SNOOZED[/]",
}


def _render_listing(listing: Listing) -> str:
    """Build Rich-formatted detail text for a listing."""
    src_color = SOURCE_COLORS.get(listing.source_id, "white")
    source = f"[{src_color} bold]{listing.source_id.upper()}[/]"
    status = STATUS_LABELS.get(listing.status, listing.status)
    price = f"[bold]${listing.price:,.2f}[/]" if listing.price else "[dim]no price[/]"

    # Title block
    lines = [
        f"[bold]{listing.title}[/]",
        f"{price}  {source}  {status}",
    ]

    # Location
    if listing.location:
        lines.append(f"[dim]{listing.location}[/]")

    lines.append("")

    # URL
    lines.append(f"[dim underline]{listing.url}[/]")

    # AI evaluation
    if listing.ai_evaluation:
        try:
            ev = json.loads(listing.ai_evaluation)
            if ev.get("notable") or ev.get("reason"):
                lines.append("")
                lines.append("[bold magenta]AI NOTES[/]")
                if ev.get("notable"):
                    lines.append(f"  [magenta]★[/] {ev['notable']}")
                if ev.get("reason"):
                    lines.append(f"  {ev['reason']}")
        except Exception:
            pass

    # Description
    if listing.description:
        lines.append("")
        lines.append(listing.description)

    # Key hints
    lines.append("")
    lines.append(
        "[dim]\\[o][/] open  "
        "[dim]\\[s][/] save  "
        "[dim]\\[d][/] dismiss  "
        "[dim]\\[n][/] snooze"
    )

    return "\n".join(lines)


class DetailPanel(Widget):
    can_focus = True

    BINDINGS = [
        ("o", "open_url", "Open"),
        ("s", "save_listing", "Save"),
        ("d", "dismiss_listing", "Dismiss"),
        ("n", "snooze_listing", "Snooze"),
    ]

    DEFAULT_CSS = """
    DetailPanel {
        width: 100%;
        height: 100%;
        background: $surface;
        border-left: tall transparent;
    }
    DetailPanel:focus { border-left: tall $accent; }
    DetailPanel #detail-header {
        dock: top;
        height: 3;
        padding: 1 1 0 1;
        color: $text-muted;
        text-style: bold;
        background: $surface;
    }
    DetailPanel VerticalScroll { height: 1fr; padding: 0 2; }
    DetailPanel #hero-image { height: 15; width: auto; }
    DetailPanel #detail-content { width: 100%; padding: 1 0; }
    """

    def __init__(self) -> None:
        super().__init__()
        self._listing: Listing | None = None
        self._thumbnail_cache = ThumbnailCache()

    def compose(self) -> ComposeResult:
        yield Static("DETAIL", id="detail-header")
        with VerticalScroll():
            if HAS_IMAGE_WIDGET:
                yield KittyImage("", id="hero-image")
            yield Static("[dim]Select a listing[/]", id="detail-content")

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
        header = self.query_one("#detail-header", Static)
        if listing is None:
            content.update("[dim]Select a listing[/]")
            header.update("DETAIL")
            self._clear_image()
            return
        header.update(f"DETAIL  [dim]{listing.source_id.upper()}[/]")
        content.update(_render_listing(listing))
        if listing.image_urls:
            self.run_worker(self._load_image(listing.image_urls[0]), exclusive=True)
        else:
            self._clear_image()

    async def _load_image(self, url: str) -> None:
        result = await self._thumbnail_cache.get(url)
        if isinstance(result, Path) and HAS_IMAGE_WIDGET:
            try:
                image_widget = self.query_one("#hero-image", KittyImage)
                image_widget.image = str(result)
            except Exception as e:
                logger.debug("Failed to render hero image: %s", e)
        else:
            self._clear_image()

    def _clear_image(self) -> None:
        if HAS_IMAGE_WIDGET:
            try:
                image_widget = self.query_one("#hero-image", KittyImage)
                image_widget.image = ""
            except Exception:
                pass

    def _mark_status(self, status: str) -> None:
        if not self._listing:
            return
        listing_id = self._listing.id
        data_layer = getattr(self.app, "_data_layer", None)
        if not data_layer:
            return
        async def _do() -> None:
            await data_layer.mark_status(listing_id, status)
            await self.app._poll()  # type: ignore[attr-defined]
        self.app.run_worker(_do(), exclusive=False)

    def action_open_url(self) -> None:
        if not self._listing:
            return
        import subprocess, sys
        opener = "open" if sys.platform == "darwin" else "xdg-open"
        subprocess.Popen([opener, self._listing.url])

    def action_save_listing(self) -> None:
        self._mark_status("saved")

    def action_dismiss_listing(self) -> None:
        self._mark_status("dismissed")

    def action_snooze_listing(self) -> None:
        self._mark_status("snoozed")
