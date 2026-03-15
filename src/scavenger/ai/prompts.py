from scavenger.models import Profile, Listing


def build_prompt(profile: Profile, listing: Listing) -> tuple[str, str]:
    """Return (system_prompt, user_prompt) for the filter evaluation call."""
    keywords_str = ", ".join(
        kw if isinstance(kw, str) else " or ".join(kw)
        for kw in profile.keywords
    )
    price_range = (
        f"${profile.price_min or 0:.0f} – ${profile.price_max:.0f}"
        if profile.price_max
        else "any price"
    )

    system = f"""You are an expert in {profile.name}. A collector is searching for items matching their profile.
Evaluate the listing and respond ONLY with valid JSON matching this exact schema:
{{"relevant": bool, "reason": "string", "notable": "string or null", "escalate": bool}}

Interest profile:
- Keywords (must match): {keywords_str}
- Negative keywords (any match = not relevant): {", ".join(profile.negative_keywords)}
- Price range: {price_range}

Rules:
- relevant: false if any negative keyword appears or the item clearly does not match the profile
- reason: 1-2 sentences explaining your decision; shown directly to the user
- notable: only if there is something specific worth highlighting — rare variant, seller misidentification, significant underpricing, unusual condition. Set to null if nothing notable.
- escalate: true only if this is an unusually good opportunity the collector should see immediately"""

    price_str = f"${listing.price:.2f}" if listing.price else "price not listed"
    user = f"""Title: {listing.title}
Price: {price_str}
Description: {listing.description or "(no description)"}"""

    return system, user
