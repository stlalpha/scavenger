from textual.app import ComposeResult
from textual.screen import Screen
from textual.containers import Horizontal, Vertical
from scavenger.tui.widgets.profile_sidebar import ProfileSidebar
from scavenger.tui.widgets.results_feed import ResultsFeed
from scavenger.tui.widgets.detail_panel import DetailPanel
from scavenger.tui.widgets.log_panel import LogPanel
from scavenger.tui.widgets.status_bar import StatusBar
from scavenger.tui.widgets.splitter import VSplitter, HSplitter
from scavenger.tui.messages import ListingSelected, ListingOpened, ProfileSelected, DataUpdated
from scavenger.models import Profile


class MainScreen(Screen):
    DEFAULT_CSS = """
    MainScreen {
        layout: vertical;
        background: #1a1a1a;
    }
    MainScreen #main-columns {
        height: 1fr;
    }

    #left-side {
        width: 60%;
        min-width: 50;
    }
    #left-top {
        height: 1fr;
    }
    #left-top #pane-sidebar {
        width: 22;
        min-width: 14;
    }
    #left-top #pane-feed {
        width: 1fr;
        min-width: 30;
    }
    #pane-log {
        height: 12;
        min-height: 3;
    }

    #pane-detail {
        width: 40%;
        min-width: 30;
    }
    """

    def __init__(self, profiles: list[Profile]) -> None:
        super().__init__()
        self._profiles = profiles

    def compose(self) -> ComposeResult:
        with Horizontal(id="main-columns"):
            with Vertical(id="left-side"):
                with Horizontal(id="left-top"):
                    yield ProfileSidebar(profiles=self._profiles, id="pane-sidebar")
                    yield VSplitter("pane-sidebar", "pane-feed", id="vsplit1")
                    yield ResultsFeed(id="pane-feed")
                yield HSplitter("left-top", "pane-log", id="hsplit1")
                yield LogPanel(id="pane-log")
            yield VSplitter("left-side", "pane-detail", id="vsplit2")
            yield DetailPanel(id="pane-detail")
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
        await feed.update_listings(event.listings, from_poll=True)
        self.query_one(ProfileSidebar).update_stats(event.profile_stats)
        self.query_one(StatusBar).set_new_count(sum(event.profile_stats.values()))
        self.query_one(DetailPanel).show_listing(feed.focused_listing)
