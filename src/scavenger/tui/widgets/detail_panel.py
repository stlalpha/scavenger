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

# Import textual-image; fall back gracefully if unavailable
try:
    from textual_image.widget import Image as KittyImage
    HAS_IMAGE_WIDGET = True
except ImportError:
    HAS_IMAGE_WIDGET = False


class DetailPanel(Widget):
    DEFAULT_CSS = """
    DetailPanel { width: 100%; height: 100%; padding: 1; }
    DetailPanel VerticalScroll { height: 100%; }
    DetailPanel #hero-image { height: 20; width: 100%; }
    DetailPanel #detail-content { width: 100%; }
    """

    def __init__(self) -> None:
        super().__init__()
        self._listing: Listing | None = None
        self._thumbnail_cache = ThumbnailCache()

    def compose(self) -> ComposeResult:
        with VerticalScroll():
            if HAS_IMAGE_WIDGET:
                yield KittyImage("", id="hero-image")
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
            self._clear_image()
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
        # Load hero image async
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
