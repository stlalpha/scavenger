# tests/test_models.py
from datetime import datetime, timezone
import pytest
from scavenger.models import Listing, Profile

def test_listing_defaults():
    now = datetime.now(timezone.utc)
    listing = Listing(
        id="abc123",
        profile_id="p1",
        source_id="ebay",
        title="Sony 85mm f/1.4",
        url="https://www.ebay.com/itm/123",
        first_seen=now,
        last_seen=now,
        relevance_score=75.0,
    )
    assert listing.status == "new"
    assert listing.currency == "USD"
    assert listing.price is None

def test_listing_requires_id():
    with pytest.raises(Exception):
        Listing(title="oops")

def test_profile_keyword_structure():
    profile = Profile(
        id="p1",
        name="Sony Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken"],
        sources=["ebay"],
    )
    assert profile.poll_interval_sec == 900
    assert profile.enabled is True
    assert profile.alert_priority == "normal"

def test_profile_invalid_alert_priority():
    with pytest.raises(Exception):
        Profile(
            id="p1",
            name="Bad",
            keywords=["test"],
            negative_keywords=[],
            sources=["ebay"],
            alert_priority="invalid",
        )

def test_profile_poll_interval_minimum():
    with pytest.raises(Exception):
        Profile(
            id="p1",
            name="Fast",
            keywords=["test"],
            negative_keywords=[],
            sources=["ebay"],
            poll_interval_sec=5,
        )
