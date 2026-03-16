from textual.app import ComposeResult
from textual.screen import Screen
from textual.containers import Horizontal
from scavenger.tui.widgets.profile_sidebar import ProfileSidebar
from scavenger.tui.widgets.results_feed import ResultsFeed
from scavenger.tui.widgets.detail_panel import DetailPanel
from scavenger.tui.widgets.status_bar import StatusBar
from scavenger.tui.messages import ListingSelected, ProfileSelected, DataUpdated
from scavenger.models import Profile


class MainScreen(Screen):
    DEFAULT_CSS = """
    MainScreen {
        layout: vertical;
    }
    MainScreen #main-columns {
        layout: horizontal;
        height: 1fr;
    }
    MainScreen ProfileSidebar { width: 1fr; }
    MainScreen ResultsFeed { width: 2fr; }
    MainScreen DetailPanel { width: 2fr; }
    """

    def __init__(self, profiles: list[Profile]) -> None:
        super().__init__()
        self._profiles = profiles

    def compose(self) -> ComposeResult:
        with Horizontal(id="main-columns"):
            yield ProfileSidebar(profiles=self._profiles)
            yield ResultsFeed()
            yield DetailPanel()
        yield StatusBar()

    def on_listing_selected(self, event: ListingSelected) -> None:
        self.query_one(DetailPanel).show_listing(event.listing)

    def on_profile_selected(self, event: ProfileSelected) -> None:
        self.app.set_active_profile(event.profile_id)  # type: ignore[attr-defined]

    def on_data_updated(self, event: DataUpdated) -> None:
        feed = self.query_one(ResultsFeed)
        self.query_one(ResultsFeed).update_listings(event.listings)
        self.query_one(ProfileSidebar).update_stats(event.profile_stats)
        self.query_one(StatusBar).set_new_count(sum(event.profile_stats.values()))
        # Refresh detail panel with updated listing data
        self.query_one(DetailPanel).show_listing(feed.focused_listing)
