import re
from scavenger.models import Profile


def _contains(text: str, term: str) -> bool:
    return bool(re.search(re.escape(term), text, re.IGNORECASE))


def _group_matches(text: str, keyword: str | list[str]) -> bool:
    if isinstance(keyword, list):
        return any(_contains(text, term) for term in keyword)
    return _contains(text, keyword)


def score_listing(
    profile: Profile, title: str, description: str, price: float | None
) -> float:
    combined = f"{title} {description}"

    for neg in profile.negative_keywords:
        if _contains(combined, neg):
            return 0.0

    total = len(profile.keywords)
    if total == 0:
        return 0.0

    combined_hits = sum(1 for kw in profile.keywords if _group_matches(combined, kw))
    if combined_hits < total:
        return 0.0

    title_hits = sum(1 for kw in profile.keywords if _group_matches(title, kw))
    desc_hits = sum(1 for kw in profile.keywords if _group_matches(description, kw))

    title_score = (title_hits / total) * 40.0
    extra_desc = max(0, desc_hits - title_hits)
    desc_score = (min(extra_desc, total) / total) * 20.0

    price_score = 0.0
    if price is None or (profile.price_min is None and profile.price_max is None):
        price_score = 20.0
    elif (profile.price_min is None or price >= profile.price_min) and \
         (profile.price_max is None or price <= profile.price_max):
        price_score = 20.0

    return min(100.0, title_score + desc_score + price_score + 20.0)
