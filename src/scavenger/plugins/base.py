from typing import Protocol, runtime_checkable
from scavenger.models import Profile, Listing


class BotDetectedError(Exception):
    """Raised when a marketplace blocks the scraper and needs manual intervention."""
    def __init__(self, plugin_id: str, url: str, message: str = ""):
        self.plugin_id = plugin_id
        self.url = url
        super().__init__(message or f"{plugin_id}: bot detection — manual verification needed")


@runtime_checkable
class Plugin(Protocol):
    plugin_id: str

    async def fetch(self, profile: Profile) -> list[Listing]: ...
    # NOTE: supports_geo is async to match the Protocol definition consistently across all
    # plugins. Do not change to a sync method — it would break the Protocol contract.
    async def supports_geo(self) -> bool: ...
