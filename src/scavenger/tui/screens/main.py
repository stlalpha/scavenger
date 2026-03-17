from textual.app import ComposeResult
from textual.screen import Screen
from textual.containers import Horizontal, Vertical
from scavenger.tui.widgets.profile_sidebar import ProfileSidebar
from scavenger.tui.widgets.results_feed import ResultsFeed
from scavenger.tui.widgets.detail_panel import DetailPanel
from scavenger.tui.widgets.log_panel import LogPanel
from scavenger.tui.widgets.status_bar import StatusBar
from scavenger.tui.messages import ListingSelected, ListingOpened, ProfileSelected, DataUpdated
from scavenger.models import Profile


class MainScreen(Screen):
    DEFAULT_CSS = """
    MainScreen {
        layout: vertical;
        background: $background;
    }
    MainScreen #main-columns {
        height: 1fr;
    }

    /* Left side: profiles + listings on top, log on bottom */
    #left-side {
        width: 1fr;
        min-width: 50;
    }
    #left-top {
        height: 1fr;
    }
    #left-top ProfileSidebar {
        width: 1fr;
        min-width: 20;
        max-width: 30;
        border-right: vkey $panel-darken-2;
    }
    #left-top ResultsFeed {
        width: 2fr;
        min-width: 30;
    }

    /* Log panel fills bottom of left side */
    #left-side LogPanel {
        height: 1fr;
        border-top: hkey $panel-darken-2;
    }

    /* Right side: detail panel */
    MainScreen DetailPanel {
        width: 1fr;
        min-width: 40;
        border-left: vkey $panel-darken-2;
    }
    """

    def __init__(self, profiles: list[Profile]) -> None:
        super().__init__()
        self._profiles = profiles

    def compose(self) -> ComposeResult:
        with Horizontal(id="main-columns"):
            with Vertical(id="left-side"):
                with Horizontal(id="left-top"):
                    yield ProfileSidebar(profiles=self._profiles)
                    yield ResultsFeed()
                yield LogPanel()
            yield DetailPanel()
        yield StatusBar()

    def on_listing_selected(self, event: ListingSelected) -> None:
        self.query_one(DetailPanel).show_listing(event.listing)

    def on_listing_opened(self, event: ListingOpened) -> None:
        if event.listing.status == "new":
            data_layer = getattr(self.app, "_data_layer", None)
            if data_layer:
                async def _mark() -> None:
                    await data_layer.mark_seen(event.listing.id)
                self.app.run_worker(_mark(), exclusive=False)

    def on_profile_selected(self, event: ProfileSelected) -> None:
        self.app.set_active_profile(event.profile_id)  # type: ignore[attr-defined]
        feed = self.query_one(ResultsFeed)
        feed.invalidate_fingerprint()
        self.query_one(DetailPanel).show_listing(None)

        async def _switch() -> None:
            await feed.update_listings([])
            await self.app._poll()  # type: ignore[attr-defined]
        self.app.run_worker(_switch(), exclusive=True)

    async def on_data_updated(self, event: DataUpdated) -> None:
        feed = self.query_one(ResultsFeed)
        await feed.update_listings(event.listings)
        self.query_one(ProfileSidebar).update_stats(event.profile_stats)
        self.query_one(StatusBar).set_new_count(sum(event.profile_stats.values()))
        self.query_one(DetailPanel).show_listing(feed.focused_listing)
