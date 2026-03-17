import logging
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import ListItem, ListView, Label, Static
from scavenger.models import Profile
from scavenger.tui.messages import ProfileSelected

logger = logging.getLogger(__name__)

SOURCE_GLYPHS = {"ebay": "eb", "craigslist": "cl", "facebook": "fb"}


class ProfileSidebar(Widget):
    DEFAULT_CSS = """
    ProfileSidebar {
        width: 100%;
        height: 100%;
        background: $surface;
    }
    ProfileSidebar #sidebar-header {
        dock: top;
        height: 3;
        padding: 1 1 0 1;
        color: $text-muted;
        text-style: bold;
        background: $surface;
    }
    ProfileSidebar ListView {
        height: 1fr;
        background: transparent;
        padding: 0 1;
    }
    ProfileSidebar ListView > ListItem {
        padding: 0 1;
        height: auto;
    }
    ProfileSidebar ListView > ListItem.--highlight {
        background: $boost;
    }
    """

    def __init__(self, profiles: list[Profile]) -> None:
        super().__init__()
        self._profiles = profiles
        self._stats: dict[str, int] = {}

    def compose(self) -> ComposeResult:
        yield Static("PROFILES", id="sidebar-header")
        yield ListView(*[
            ListItem(Label(self._label(p)), id=f"profile-{p.id}")
            for p in self._profiles
        ])

    def _label(self, profile: Profile) -> str:
        count = self._stats.get(profile.id, 0)
        badge = f" [bold cyan]({count})[/]" if count > 0 else ""
        sources = " ".join(
            f"[dim]{SOURCE_GLYPHS.get(s, s[:2])}[/]"
            for s in profile.sources
        )
        state = "[dim]off[/] " if not profile.enabled else ""
        return f"{state}{profile.name}{badge}\n  {sources}"

    def update_stats(self, stats: dict[str, int]) -> None:
        self._stats = stats
        self._refresh_labels()

    def _refresh_labels(self) -> None:
        list_view = self.query_one(ListView)
        for i, profile in enumerate(self._profiles):
            try:
                item = list_view.query(ListItem)[i]
                item.query_one(Label).update(self._label(profile))
            except Exception as e:
                logger.debug("Could not refresh label for profile %s: %s", profile.id, e)

    def get_unread(self, profile_id: str) -> int:
        return self._stats.get(profile_id, 0)

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        item_id = event.item.id or ""
        if item_id.startswith("profile-"):
            profile_id = item_id.removeprefix("profile-")
            self.post_message(ProfileSelected(profile_id=profile_id))
