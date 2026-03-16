import logging
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import ListItem, ListView, Label
from scavenger.models import Profile
from scavenger.tui.messages import ProfileSelected

logger = logging.getLogger(__name__)


class ProfileSidebar(Widget):
    DEFAULT_CSS = """
    ProfileSidebar {
        width: 100%;
        height: 100%;
        border-right: solid $panel-darken-1;
    }
    ProfileSidebar ListView {
        height: 100%;
        background: transparent;
    }
    """

    def __init__(self, profiles: list[Profile]) -> None:
        super().__init__()
        self._profiles = profiles
        self._stats: dict[str, int] = {}

    def compose(self) -> ComposeResult:
        yield ListView(*[
            ListItem(Label(self._label(p)), id=f"profile-{p.id}")
            for p in self._profiles
        ])

    def _label(self, profile: Profile) -> str:
        count = self._stats.get(profile.id, 0)
        badge = f" ({count})" if count > 0 else ""
        return f"{profile.name}{badge}"

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
