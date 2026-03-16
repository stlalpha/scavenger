import pytest
from textual.app import App, ComposeResult
from scavenger.tui.widgets.profile_sidebar import ProfileSidebar
from scavenger.tui.widgets.results_feed import ResultsFeed
from scavenger.tui.widgets.detail_panel import DetailPanel
from scavenger.tui.widgets.status_bar import StatusBar
from scavenger.models import Profile, Listing
from datetime import datetime, timezone


def make_profiles() -> list[Profile]:
    return [
        Profile(id="p1", name="Sony Glass", keywords=["sony"],
                negative_keywords=[], sources=["ebay"]),
        Profile(id="p2", name="IBM AS/400", keywords=["as400"],
                negative_keywords=[], sources=["ebay"]),
    ]


def make_listings(n: int = 3) -> list[Listing]:
    now = datetime.now(timezone.utc)
    return [
        Listing(
            id=f"id{i}", profile_id="p1", source_id="ebay",
            title=f"Sony Lens {i}", url=f"https://ebay.com/{i}",
            first_seen=now, last_seen=now, relevance_score=80.0,
            price=float(100 + i * 50),
        )
        for i in range(n)
    ]


class SidebarTestApp(App):
    def __init__(self, profiles):
        super().__init__()
        self._profiles = profiles
    def compose(self) -> ComposeResult:
        yield ProfileSidebar(profiles=self._profiles)


async def test_sidebar_renders():
    app = SidebarTestApp(make_profiles())
    async with app.run_test(size=(40, 20)) as pilot:
        await pilot.pause(0.1)
        assert app.query_one(ProfileSidebar) is not None


async def test_sidebar_unread_count():
    app = SidebarTestApp(make_profiles())
    async with app.run_test(size=(40, 20)) as pilot:
        await pilot.pause(0.1)
        sidebar = app.query_one(ProfileSidebar)
        sidebar.update_stats({"p1": 5, "p2": 0})
        assert sidebar.get_unread("p1") == 5
        assert sidebar.get_unread("p2") == 0


class FeedTestApp(App):
    def compose(self) -> ComposeResult:
        yield ResultsFeed()
    async def on_mount(self) -> None:
        await self.query_one(ResultsFeed).update_listings(make_listings(3))


async def test_feed_renders_listings():
    app = FeedTestApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        assert app.query_one(ResultsFeed).listing_count == 3


async def test_feed_j_moves_cursor_down():
    app = FeedTestApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        feed = app.query_one(ResultsFeed)
        assert feed.cursor == 0
        await pilot.press("j")
        assert feed.cursor == 1


async def test_feed_down_arrow_moves_cursor():
    app = FeedTestApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.3)
        await pilot.press("down")
        await pilot.pause(0.1)
        assert app.query_one(ResultsFeed).cursor == 1


async def test_feed_k_moves_cursor_up():
    app = FeedTestApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        await pilot.press("j")
        await pilot.press("k")
        assert app.query_one(ResultsFeed).cursor == 0


async def test_feed_cursor_floors_at_zero():
    app = FeedTestApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        await pilot.press("k")
        assert app.query_one(ResultsFeed).cursor == 0


async def test_feed_notable_star():
    now = datetime.now(timezone.utc)
    listings = [Listing(
        id="x", profile_id="p1", source_id="ebay",
        title="Zeiss Lens", url="https://ebay.com/x",
        first_seen=now, last_seen=now, relevance_score=90.0,
        ai_evaluation='{"relevant": true, "reason": "Great", "notable": "Zeiss variant", "escalate": false}',
    )]
    class StarApp(App):
        def compose(self) -> ComposeResult:
            yield ResultsFeed()
        async def on_mount(self) -> None:
            await self.query_one(ResultsFeed).update_listings(listings)
    app = StarApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        assert app.query_one(ResultsFeed).has_notable("x") is True


class DetailTestApp(App):
    def __init__(self, listing):
        super().__init__()
        self._listing = listing
    def compose(self) -> ComposeResult:
        yield DetailPanel()
    def on_mount(self) -> None:
        self.query_one(DetailPanel).show_listing(self._listing)


async def test_detail_shows_listing():
    now = datetime.now(timezone.utc)
    listing = Listing(
        id="abc", profile_id="p1", source_id="ebay",
        title="Sony 85mm f/1.4", url="https://ebay.com/1",
        first_seen=now, last_seen=now, relevance_score=80.0,
        price=249.99, description="Excellent condition.",
    )
    app = DetailTestApp(listing)
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        assert app.query_one(DetailPanel).current_listing.id == "abc"


async def test_detail_has_ai_notes():
    now = datetime.now(timezone.utc)
    listing = Listing(
        id="z", profile_id="p1", source_id="ebay",
        title="Sony Zeiss", url="https://ebay.com/z",
        first_seen=now, last_seen=now, relevance_score=90.0,
        ai_evaluation='{"relevant": true, "reason": "Strong match", "notable": "Zeiss variant", "escalate": false}',
    )
    app = DetailTestApp(listing)
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        assert app.query_one(DetailPanel).has_ai_notes is True


async def test_detail_none_clears_panel():
    class EmptyApp(App):
        def compose(self) -> ComposeResult:
            yield DetailPanel()
    app = EmptyApp()
    async with app.run_test(size=(80, 30)) as pilot:
        await pilot.pause(0.1)
        assert app.query_one(DetailPanel).current_listing is None


class StatusBarTestApp(App):
    def compose(self) -> ComposeResult:
        yield StatusBar()


async def test_status_bar_renders():
    app = StatusBarTestApp()
    async with app.run_test(size=(120, 5)) as pilot:
        await pilot.pause(0.1)
        assert app.query_one(StatusBar) is not None


async def test_status_bar_daemon_unreachable():
    app = StatusBarTestApp()
    async with app.run_test(size=(120, 5)) as pilot:
        await pilot.pause(0.1)
        bar = app.query_one(StatusBar)
        bar.set_daemon_status(reachable=False)
        assert bar.daemon_reachable is False


async def test_status_bar_new_count():
    app = StatusBarTestApp()
    async with app.run_test(size=(120, 5)) as pilot:
        await pilot.pause(0.1)
        bar = app.query_one(StatusBar)
        bar.set_new_count(7)
        assert bar.new_today == 7
