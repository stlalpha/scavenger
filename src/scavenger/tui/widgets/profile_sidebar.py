import logging
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import ListItem, ListView, Label, Static
from scavenger.models import Profile
from scavenger.tui.messages import ProfileSelected

logger = logging.getLogger(__name__)

# ░▒▓ inspired but modern
SRC = {"ebay": ("eb", "#e6db74"), "craigslist": ("cl", "#f92672"), "facebook": ("fb", "#66d9ef")}


class ProfileSidebar(Widget):
    DEFAULT_CSS = """
    ProfileSidebar {
        width: 100%;
        height: 100%;
        background: #1c1c1c;
    }
    ProfileSidebar #sidebar-hdr {
        dock: top;
        height: 1;
        padding: 0 1;
        background: #1c1c1c;
        color: #a6e22e;
        text-style: bold;
    }
    ProfileSidebar ListView {
        height: 1fr;
        background: transparent;
        padding: 1 0;
    }
    ProfileSidebar ListView > ListItem {
        padding: 0 1;
        height: auto;
        background: transparent;
    }
    ProfileSidebar ListView > ListItem.--highlight {
        background: #272727;
    }
    """

    def __init__(self, profiles: list[Profile], **kwargs) -> None:
        super().__init__(**kwargs)
        self._profiles = profiles
        self._stats: dict[str, int] = {}
        self._daemon_profiles: set[str] = set()

    def compose(self) -> ComposeResult:
        yield Static(" PROFILES", id="sidebar-hdr")
        yield ListView(*[
            ListItem(Label(self._label(p)), id=f"profile-{p.id}")
            for p in self._profiles
        ])

    def _label(self, profile: Profile) -> str:
        count = self._stats.get(profile.id, 0)
        badge = f" [bold #a6e22e]{count}[/]" if count > 0 else ""
        srcs = " ".join(f"[{SRC.get(s, ('??','#75715e'))[1]}]{SRC.get(s, ('??','#75715e'))[0]}[/]" for s in profile.sources)
        if not profile.enabled:
            return f"  [#75715e]{profile.name}[/] [dim]off[/]"
        if self._daemon_profiles and profile.id not in self._daemon_profiles:
            return f"  [#fd971f]▪[/] {profile.name}{badge} [dim]{srcs}[/]"
        indicator = "[#a6e22e]▸[/]" if count > 0 else "[#3a3a3a]▸[/]"
        return f" {indicator} {profile.name}{badge} [dim]{srcs}[/]"

    def update_stats(self, stats: dict[str, int]) -> None:
        self._stats = stats
        self._refresh_labels()

    def set_daemon_profiles(self, profile_ids: list[str]) -> None:
        self._daemon_profiles = set(profile_ids)
        self._refresh_labels()

    def _refresh_labels(self) -> None:
        list_view = self.query_one(ListView)
        for i, profile in enumerate(self._profiles):
            try:
                item = list_view.query(ListItem)[i]
                item.query_one(Label).update(self._label(profile))
            except Exception as e:
                logger.debug("Could not refresh label for profile %s: %s", profile.id, e)

    async def add_profile(self, profile: Profile) -> None:
        self._profiles.append(profile)
        list_view = self.query_one(ListView)
        await list_view.append(
            ListItem(Label(self._label(profile)), id=f"profile-{profile.id}")
        )

    async def rebuild(self, profiles: list[Profile]) -> None:
        self._profiles = list(profiles)
        list_view = self.query_one(ListView)
        await list_view.clear()
        for p in self._profiles:
            await list_view.append(
                ListItem(Label(self._label(p)), id=f"profile-{p.id}")
            )

    def get_unread(self, profile_id: str) -> int:
        return self._stats.get(profile_id, 0)

    def on_list_view_highlighted(self, event: ListView.Highlighted) -> None:
        if event.item is None:
            return
        item_id = event.item.id or ""
        if item_id.startswith("profile-"):
            self.post_message(ProfileSelected(profile_id=item_id.removeprefix("profile-")))

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        item_id = event.item.id or ""
        if item_id.startswith("profile-"):
            self.post_message(ProfileSelected(profile_id=item_id.removeprefix("profile-")))
