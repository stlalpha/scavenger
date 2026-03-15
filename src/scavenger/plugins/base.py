from typing import Protocol, runtime_checkable
from scavenger.models import Profile, Listing


@runtime_checkable
class Plugin(Protocol):
    plugin_id: str

    async def fetch(self, profile: Profile) -> list[Listing]: ...
    async def supports_geo(self) -> bool: ...
