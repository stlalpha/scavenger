import pytest
from scavenger.scoring import score_listing
from scavenger.models import Profile


@pytest.fixture
def sony_profile():
    return Profile(
        id="sony", name="Sony Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken", "fungus", "parts only"],
        sources=["ebay"],
        price_min=50.0, price_max=800.0,
    )


def test_strong_title_match_scores_high(sony_profile):
    score = score_listing(sony_profile, "Sony A-mount 85mm f/1.4 lens", "Great condition", 250.0)
    assert score >= 70.0

def test_negative_keyword_scores_zero(sony_profile):
    score = score_listing(sony_profile, "Sony A-mount lens broken", "parts only", 50.0)
    assert score == 0.0

def test_no_keyword_match_scores_zero(sony_profile):
    score = score_listing(sony_profile, "Canon EF 50mm lens", "Great Canon lens", 200.0)
    assert score == 0.0

def test_price_outside_band_reduces_score(sony_profile):
    in_band = score_listing(sony_profile, "Sony A-mount lens", "", 300.0)
    out_of_band = score_listing(sony_profile, "Sony A-mount lens", "", 2000.0)
    assert in_band > out_of_band

def test_title_match_scores_higher_than_description_only(sony_profile):
    title_match = score_listing(sony_profile, "Sony A-mount 85mm", "", 300.0)
    desc_match = score_listing(sony_profile, "Camera lens for sale", "Sony A-mount 85mm", 300.0)
    assert title_match > desc_match
