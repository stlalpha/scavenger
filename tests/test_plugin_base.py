from scavenger.plugins.base import Plugin
from scavenger.models import Profile, Listing


class GoodPlugin:
    plugin_id = "good"
    async def fetch(self, profile: Profile) -> list[Listing]: return []
    async def supports_geo(self) -> bool: return False


class BadPlugin:
    pass


def test_good_plugin_satisfies_protocol():
    assert isinstance(GoodPlugin(), Plugin)

def test_bad_plugin_fails_protocol():
    assert not isinstance(BadPlugin(), Plugin)
