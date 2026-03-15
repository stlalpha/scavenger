from typing import Protocol, runtime_checkable
from scavenger.models import Profile, Listing


@runtime_checkable
class Plugin(Protocol):
    plugin_id: str

    async def fetch(self, profile: Profile) -> list[Listing]: ...
    # NOTE: supports_geo is async to match the Protocol definition consistently across all
    # plugins. Do not change to a sync method — it would break the Protocol contract.
    async def supports_geo(self) -> bool: ...
