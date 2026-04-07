import json
from pathlib import Path
from scavenger.ai.prompts import build_prompt
from scavenger.models import Profile, Listing
from datetime import datetime, timezone

FIXTURES = Path(__file__).parent / "fixtures"


def make_profile() -> Profile:
    return Profile(
        id="sony", name="Sony A-mount Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken", "fungus"],
        sources=["ebay"],
        price_min=50.0, price_max=800.0,
    )


def make_listing() -> Listing:
    now = datetime.now(timezone.utc)
    return Listing(
        id="abc", profile_id="sony", source_id="ebay",
        title="Sony 85mm f/1.4 A-mount",
        description="Great condition, no fungus. $249.99",
        price=249.99,
        url="https://ebay.com/1",
        first_seen=now, last_seen=now, relevance_score=80.0,
    )


def test_system_prompt_contains_profile_name():
    system, _ = build_prompt(make_profile(), make_listing())
    assert "Sony A-mount Glass" in system


def test_system_prompt_contains_keywords():
    system, _ = build_prompt(make_profile(), make_listing())
    assert "sony" in system.lower()
    assert "a-mount" in system.lower()


def test_system_prompt_contains_negative_keywords():
    system, _ = build_prompt(make_profile(), make_listing())
    assert "broken" in system.lower()
    assert "fungus" in system.lower()


def test_system_prompt_contains_json_schema():
    system, _ = build_prompt(make_profile(), make_listing())
    assert "relevant" in system
    assert "notable" in system
    assert "escalate" in system


def test_user_prompt_contains_listing_details():
    _, user = build_prompt(make_profile(), make_listing())
    assert "Sony 85mm f/1.4" in user
    assert "249.99" in user
    assert "no fungus" in user


def test_price_range_in_system_prompt():
    system, _ = build_prompt(make_profile(), make_listing())
    assert "50" in system
    assert "800" in system


def test_price_range_no_min_says_up_to():
    profile_no_min = Profile(
        id="sony2", name="Sony A-mount Glass",
        keywords=["sony"], negative_keywords=[],
        sources=["ebay"], price_max=800.0,
    )
    system, _ = build_prompt(profile_no_min, make_listing())
    assert "up to" in system
    assert "800" in system
    assert "$0" not in system


def test_fixture_is_valid_ai_evaluation():
    from scavenger.ai.models import AIEvaluation
    data = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    ev = AIEvaluation(**data)
    assert ev.relevant is True
    assert ev.notable is not None


# --- price range branch coverage ---

def _make_profile(**overrides):
    from scavenger.models import Profile
    defaults = dict(
        id="p1", name="Test", keywords=["test"],
        negative_keywords=[], sources=["ebay"],
    )
    defaults.update(overrides)
    return Profile(**defaults)


def _make_listing():
    from scavenger.models import Listing
    now = datetime.now(timezone.utc)
    return Listing(
        id="x", profile_id="p1", source_id="ebay", title="Test",
        url="https://test.com", first_seen=now, last_seen=now,
        relevance_score=50.0, price=100.0,
    )


def test_price_range_with_both_bounds():
    profile = _make_profile(price_min=50.0, price_max=500.0)
    system, _ = build_prompt(profile, _make_listing())
    assert "$50" in system and "$500" in system


def test_price_range_max_only():
    profile = _make_profile(price_max=500.0)
    system, _ = build_prompt(profile, _make_listing())
    assert "up to $500" in system


def test_price_range_min_only():
    profile = _make_profile(price_min=50.0)
    system, _ = build_prompt(profile, _make_listing())
    assert "$50 and above" in system


def test_price_range_neither():
    profile = _make_profile()
    system, _ = build_prompt(profile, _make_listing())
    assert "any price" in system


def test_batch_price_range_with_both_bounds():
    from scavenger.ai.prompts import build_batch_prompt
    profile = _make_profile(price_min=50.0, price_max=500.0)
    system, _ = build_batch_prompt(profile, [_make_listing()])
    assert "$50" in system and "$500" in system
