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
    price = f"[bold #fd971f]${listing.price:,.2f}[/]" if listing.price else "[#555]no price[/]"

    lines = [
        f"[bold #f8f8f2]{listing.title}[/]",
        "",
        f"{price}  {src}  {status}",
    ]

    if listing.location:
        lines.append(f"[#75715e]{listing.location}[/]")

    lines.append("")
    lines.append(f"[#555 underline]{listing.url}[/]")

    if listing.ai_evaluation:
        try:
            ev = json.loads(listing.ai_evaluation)
            if ev.get("notable") or ev.get("reason"):
                lines.append("")
                lines.append("[#f92672 bold]AI INSIGHT[/]")
                if ev.get("notable"):
                    lines.append(f"[bold #f92672]![/] [#f8f8f2]{ev['notable']}[/]")
                if ev.get("reason"):
                    lines.append(f"[#888]{ev['reason']}[/]")
        except Exception:
            pass

    if listing.description:
        lines.append("")
        lines.append(f"[#888]{listing.description}[/]")

    lines.append("")

    if img_total > 1:
        nav = f"[#555]<[/] [#f8f8f2]{img_index + 1}/{img_total}[/] [#555]>[/]  "
    elif img_total == 1:
        nav = "[#444]1/1[/]  "
    else:
        nav = ""

    lines.append(
        f"{nav}"
        "[#fd971f]o[/][#75715e]pen[/]  "
        "[#fd971f]s[/][#75715e]ave[/]  "
        "[#fd971f]d[/][#75715e]ism[/]  "
        "[#fd971f]n[/][#75715e]ap[/]"
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
    }
    DetailPanel #detail-hdr {
        dock: top;
        height: 1;
        padding: 0 1;
        background: #1e1e1e;
        color: #fd971f;
        text-style: bold;
    }
    DetailPanel VerticalScroll { height: 1fr; padding: 1 2; }
    DetailPanel #hero-image { height: 18; width: auto; }
    DetailPanel #detail-content { width: 100%; padding: 1 0; }
    """

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._listing: Listing | None = None
        self._thumbnail_cache = ThumbnailCache()
        self._images: list[str] = []
        self._img_idx: int = 0
        self._pending_scrape_id: str | None = None

    def compose(self) -> ComposeResult:
        yield Static(" DETAIL", id="detail-hdr")
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
        # Skip re-render if same listing — avoids flash on poll cycle
        if listing is not None and self._listing is not None and listing.id == self._listing.id:
            return
        self._listing = listing
        self._images = []
        self._img_idx = 0
        content = self.query_one("#detail-content", Static)
        hdr = self.query_one("#detail-hdr", Static)
        if listing is None:
            content.update("[#75715e italic]  select a listing[/]")
            hdr.update(" DETAIL")
            self._clear_image()
            return
        clr = SRC_CLR.get(listing.source_id, "#75715e")
        hdr.update(f" DETAIL [{clr}]{listing.source_id.upper()}[/]")

        # Clear stale image immediately before loading new one
        self._clear_image()

        # Start with the thumbnail from the search results
        if listing.image_urls:
            self._images = list(listing.image_urls)
            self.run_worker(self._load_image_for(listing.id), exclusive=True, group="detail-images")
        content.update(_render(listing, self._img_idx, len(self._images)))

        # Fetch full gallery after a debounce — don't scrape while user is arrowing through
        if listing.url and listing.url.startswith("http"):
            self._pending_scrape_id = listing.id
            self.run_worker(self._debounced_fetch(listing.url, listing.id), exclusive=True, group="detail-gallery")

    async def _load_image_for(self, listing_id: str) -> None:
        """Load current image, bailing if listing changed."""
        if self._listing is None or self._listing.id != listing_id:
            return
        await self._load_current_image()

    async def _debounced_fetch(self, url: str, listing_id: str) -> None:
        """Wait 1.5s before scraping — if user moved on, abort."""
        import asyncio
        await asyncio.sleep(1.5)
        if self._listing is None or self._listing.id != listing_id:
            return
        await self._fetch_detail_images(url, listing_id)

    async def _fetch_detail_images(self, url: str, listing_id: str) -> None:
        """Fetch the listing page HTML and extract all image URLs."""
        try:
            from scavenger.plugins.images import fetch_listing_images
            source_id = self._listing.source_id if self._listing else ""
            images = await fetch_listing_images(url, source_id)
            if self._listing and self._listing.id == listing_id and images:
                self._images = images
                self._img_idx = 0
                self._clear_image()
                await self._load_current_image()
                self._update_content()
        except Exception as e:
            logger.debug("Image fetch failed: %s", e)

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
            self._clear_image()
            self.run_worker(self._load_current_image(), exclusive=True, group="detail-images")
            self._update_content()

    def action_prev_image(self) -> None:
        if self._images and len(self._images) > 1:
            self._img_idx = (self._img_idx - 1) % len(self._images)
            self._clear_image()
            self.run_worker(self._load_current_image(), exclusive=True, group="detail-images")
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
        if sys.platform == "darwin":
            # Force a new Chrome window in the default profile, not the
            # scraping session running with --user-data-dir
            subprocess.Popen([
                "open", "-na", "Google Chrome",
                "--args", "--profile-directory=Default", self._listing.url,
            ])
        else:
            subprocess.Popen(["xdg-open", self._listing.url])

    def action_save_listing(self) -> None:
        self._mark_status("saved")

    def action_dismiss_listing(self) -> None:
        self._mark_status("dismissed")

    def action_snooze_listing(self) -> None:
        self._mark_status("snoozed")
