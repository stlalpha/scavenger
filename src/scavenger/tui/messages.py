from textual.message import Message
from scavenger.models import Listing


class DataUpdated(Message):
    """Posted by DataLayer when new listings are detected."""
    def __init__(self, listings: list[Listing], profile_stats: dict[str, int]) -> None:
        self.listings = listings
        self.profile_stats = profile_stats
        super().__init__()


class ListingSelected(Message):
    """Posted by ResultsFeed when the focused listing changes."""
    def __init__(self, listing: Listing | None) -> None:
        self.listing = listing
        super().__init__()


class ProfileSelected(Message):
    """Posted by ProfileSidebar when active profile changes."""
    def __init__(self, profile_id: str | None) -> None:
        self.profile_id = profile_id
        super().__init__()
