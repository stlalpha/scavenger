from scavenger.tui.messages import DataUpdated, ListingSelected, ListingOpened, ProfileSelected
from datetime import datetime, timezone
from scavenger.models import Listing


def make_listing() -> Listing:
    now = datetime.now(timezone.utc)
    return Listing(
        id="abc", profile_id="p1", source_id="ebay",
        title="Sony 85mm", url="https://ebay.com/1",
        first_seen=now, last_seen=now, relevance_score=80.0,
    )


def test_data_updated_message():
    msg = DataUpdated(listings=[make_listing()], profile_stats={"p1": 3})
    assert len(msg.listings) == 1
    assert msg.profile_stats["p1"] == 3


def test_listing_selected_message():
    listing = make_listing()
    msg = ListingSelected(listing=listing)
    assert msg.listing.id == "abc"


def test_listing_selected_none():
    msg = ListingSelected(listing=None)
    assert msg.listing is None


def test_listing_opened_message():
    listing = make_listing()
    msg = ListingOpened(listing=listing)
    assert msg.listing.id == "abc"


def test_profile_selected_message():
    msg = ProfileSelected(profile_id="p1")
    assert msg.profile_id == "p1"
