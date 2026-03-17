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

SRC_CLR = {"ebay": "#e6db74", "craigslist": "#f92672", "facebook": "#66d9ef"}
STATUS_LABEL = {
    "new": "[bold #66d9ef]NEW[/]",
    "seen": "[#75715e]SEEN[/]",
    "saved": "[#a6e22e]SAVED[/]",
    "dismissed": "[#75715e]DISMISSED[/]",
    "snoozed": "[#e6db74]SNOOZED[/]",
}


def _render(listing: Listing, img_index: int = 0, img_total: int = 0) -> str:
    clr = SRC_CLR.get(listing.source_id, "#75715e")
    src = f"[{clr} bold]{listing.source_id.upper()}[/]"
    status = STATUS_LABEL.get(listing.status, listing.status)
    price = f"[bold #fd971f]${listing.price:,.2f}[/]" if listing.price else "[#75715e]no price[/]"

    lines = [
        f"[bold #f8f8f2]{listing.title}[/]",
        "",
        f"  {price}  {src}  {status}",
    ]

    if listing.location:
        lines.append(f"  [#75715e]{listing.location}[/]")

    lines.append("")
    lines.append(f"  [#75715e underline]{listing.url}[/]")

    if listing.ai_evaluation:
        try:
            ev = json.loads(listing.ai_evaluation)
            if ev.get("notable") or ev.get("reason"):
                lines.append("")
                lines.append("[#f92672]╶─── ai insight ───╴[/]")
                if ev.get("notable"):
                    lines.append(f"  [#f92672]★[/] [#f8f8f2]{ev['notable']}[/]")
                if ev.get("reason"):
                    lines.append(f"  [#75715e]{ev['reason']}[/]")
        except Exception:
            pass

    if listing.description:
        lines.append("")
        lines.append(f"[#75715e]{listing.description}[/]")

    lines.append("")

    # Image nav hint
    if img_total > 1:
        nav = f"[#75715e]\\[<][/][#f8f8f2] {img_index + 1}/{img_total} [/][#75715e]\\[>][/]  "
    elif img_total == 1:
        nav = "[#3a3a3a]1/1[/]  "
    else:
        nav = ""

    lines.append(
        f"[#3a3a3a]╶[/] {nav}"
        "[#75715e]\\[o][/][#f8f8f2]open[/]  "
        "[#75715e]\\[s][/][#f8f8f2]save[/]  "
        "[#75715e]\\[d][/][#f8f8f2]dismiss[/]  "
        "[#75715e]\\[n][/][#f8f8f2]snooze[/]"
    )

    return "\n".join(lines)


class DetailPanel(Widget):
    can_focus = True

    BINDINGS = [
        ("o", "open_url", "Open"),
        ("s", "save_listing", "Save"),
        ("d", "dismiss_listing", "Dismiss"),
        ("n", "snooze_listing", "Snooze"),
        ("full_stop", "next_image", ">"),
        ("comma", "prev_image", "<"),
    ]

    DEFAULT_CSS = """
    DetailPanel {
        width: 100%;
        height: 100%;
        background: #1e1e1e;
        border-left: solid #333;
    }
    DetailPanel #detail-hdr {
        dock: top;
        height: 1;
        padding: 0 1;
        background: #252525;
        color: #75715e;
    }
    DetailPanel VerticalScroll { height: 1fr; padding: 1 2; }
    DetailPanel #hero-image { height: 15; width: auto; }
    DetailPanel #detail-content { width: 100%; padding: 1 0; }
    """

    def __init__(self) -> None:
        super().__init__()
        self._listing: Listing | None = None
        self._thumbnail_cache = ThumbnailCache()
        self._images: list[str] = []
        self._img_idx: int = 0
        self._fetching_detail: bool = False

    def compose(self) -> ComposeResult:
        yield Static("╶ detail", id="detail-hdr")
        with VerticalScroll():
            if HAS_IMAGE_WIDGET:
                yield KittyImage("", id="hero-image")
            yield Static("[#75715e italic]  select a listing[/]", id="detail-content")

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
        self._images = []
        self._img_idx = 0
        content = self.query_one("#detail-content", Static)
        hdr = self.query_one("#detail-hdr", Static)
        if listing is None:
            content.update("[#75715e italic]  select a listing[/]")
            hdr.update("╶ detail")
            self._clear_image()
            return
        clr = SRC_CLR.get(listing.source_id, "#75715e")
        hdr.update(f"╶ detail [{clr}]{listing.source_id}[/]")

        # Start with the thumbnail from the search results
        if listing.image_urls:
            self._images = list(listing.image_urls)
            self.run_worker(self._load_current_image(), exclusive=True)
        else:
            self._clear_image()

        content.update(_render(listing, self._img_idx, len(self._images)))

        # Fetch full gallery from the detail page in background
        if listing.url and listing.url.startswith("http"):
            self._fetching_detail = True
            self.run_worker(self._fetch_detail_images(listing.url, listing.id), exclusive=False, group="detail-images")

    async def _fetch_detail_images(self, url: str, listing_id: str) -> None:
        """Scrape the listing detail page for all images."""
        try:
            from scavenger.plugins.detail_scraper import scrape_detail_images
            images = await scrape_detail_images(url)
            # Only update if we're still showing the same listing
            if self._listing and self._listing.id == listing_id and images:
                self._images = images
                self._img_idx = 0
                await self._load_current_image()
                self._update_content()
        except Exception as e:
            logger.debug("Detail image fetch failed: %s", e)
        finally:
            self._fetching_detail = False

    def _update_content(self) -> None:
        if self._listing:
            content = self.query_one("#detail-content", Static)
            content.update(_render(self._listing, self._img_idx, len(self._images)))

    async def _load_current_image(self) -> None:
        if not self._images or not HAS_IMAGE_WIDGET:
            return
        url = self._images[self._img_idx]
        result = await self._thumbnail_cache.get(url)
        if isinstance(result, Path):
            try:
                self.query_one("#hero-image", KittyImage).image = str(result)
            except Exception as e:
                logger.debug("Failed to render image: %s", e)
        else:
            self._clear_image()

    def _clear_image(self) -> None:
        if HAS_IMAGE_WIDGET:
            try:
                self.query_one("#hero-image", KittyImage).image = ""
            except Exception:
                pass

    def action_next_image(self) -> None:
        if self._images and len(self._images) > 1:
            self._img_idx = (self._img_idx + 1) % len(self._images)
            self.run_worker(self._load_current_image(), exclusive=True)
            self._update_content()

    def action_prev_image(self) -> None:
        if self._images and len(self._images) > 1:
            self._img_idx = (self._img_idx - 1) % len(self._images)
            self.run_worker(self._load_current_image(), exclusive=True)
            self._update_content()

    def _mark_status(self, status: str) -> None:
        if not self._listing:
            return
        lid = self._listing.id
        dl = getattr(self.app, "_data_layer", None)
        if not dl:
            return
        async def _do() -> None:
            await dl.mark_status(lid, status)
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
