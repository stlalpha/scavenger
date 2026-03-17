from scavenger.models import Profile, Listing


def build_prompt(profile: Profile, listing: Listing) -> tuple[str, str]:
    """Return (system_prompt, user_prompt) for the filter evaluation call."""
    keywords_str = ", ".join(
        kw if isinstance(kw, str) else " or ".join(kw)
        for kw in profile.keywords
    )
    if profile.price_max and profile.price_min is not None:
        price_range = f"${profile.price_min:.0f} – ${profile.price_max:.0f}"
    elif profile.price_max:
        price_range = f"up to ${profile.price_max:.0f}"
    elif profile.price_min is not None:
        price_range = f"${profile.price_min:.0f} and above"
    else:
        price_range = "any price"

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


def build_escalation_prompt(profile: Profile, listing: Listing, triggered_keywords: list[str]) -> tuple[str, str]:
    """Return (system_prompt, user_prompt) for the escalation model.

    This prompt asks the frontier model to evaluate whether seller claims
    associated with the triggered keywords are credible.
    """
    keywords_str = ", ".join(
        kw if isinstance(kw, str) else " or ".join(kw)
        for kw in profile.keywords
    )

    triggered_str = ", ".join(f'"{kw}"' for kw in triggered_keywords)

    system = f"""You are a domain expert in {profile.name} with deep knowledge of market values, product variants, rarity, and condition grading.

A listing matched the collector's escalation keywords: {triggered_str}. Your job is to evaluate whether these claims are credible.

Respond ONLY with valid JSON matching this schema:
{{"relevant": bool, "reason": "string", "notable": "string or null", "escalate": bool}}

Interest profile:
- Keywords: {keywords_str}
- Negative keywords: {", ".join(profile.negative_keywords) or "none"}

Evaluate critically:
- Is the item actually what the title claims? Look for misidentifications.
- If the listing says "rare" — is it genuinely uncommon, or is this a mass-produced item?
- If it says "mint" or describes condition — does the price match that condition?
- Is the price significantly above or below typical market value for this exact item?
- Is there anything the collector should know that isn't obvious from the title?

Be specific. Reference actual model numbers, production years, or market prices when possible.
- relevant: false only if the item clearly doesn't match the profile
- reason: 2-3 sentences with your expert assessment; this is shown directly to the collector
- notable: the key insight — what makes this worth attention OR why the seller's claims don't hold up. null only if there's nothing interesting to say.
- escalate: true if this is a genuinely good opportunity the collector should act on quickly"""

    price_str = f"${listing.price:.2f}" if listing.price else "price not listed"
    user = f"""Title: {listing.title}
Price: {price_str}
Source: {listing.source_id}
Description: {listing.description or "(no description)"}"""

    return system, user


def build_batch_prompt(profile: Profile, listings: list[Listing]) -> tuple[str, str]:
    """Return (system_prompt, user_prompt) for evaluating multiple listings at once."""
    keywords_str = ", ".join(
        kw if isinstance(kw, str) else " or ".join(kw)
        for kw in profile.keywords
    )
    if profile.price_max and profile.price_min is not None:
        price_range = f"${profile.price_min:.0f} – ${profile.price_max:.0f}"
    elif profile.price_max:
        price_range = f"up to ${profile.price_max:.0f}"
    elif profile.price_min is not None:
        price_range = f"${profile.price_min:.0f} and above"
    else:
        price_range = "any price"

    system = f"""You are an expert in {profile.name}. A collector is searching for items matching their profile.
Evaluate EACH listing and respond ONLY with a valid JSON array. Each element must match this schema:
{{"id": "string", "relevant": bool, "reason": "string", "notable": "string or null", "escalate": bool}}

Interest profile:
- Keywords (must match): {keywords_str}
- Negative keywords (any match = not relevant): {", ".join(profile.negative_keywords)}
- Price range: {price_range}

Rules:
- relevant: false if any negative keyword appears or the item clearly does not match the profile
- reason: 1-2 sentences explaining your decision; shown directly to the user
- notable: only if there is something specific worth highlighting — rare variant, seller misidentification, significant underpricing, unusual condition. Set to null if nothing notable.
- escalate: true only if this is an unusually good opportunity the collector should see immediately
- Return one object per listing in the same order, using the listing's id field"""

    listing_blocks = []
    for listing in listings:
        price_str = f"${listing.price:.2f}" if listing.price else "price not listed"
        listing_blocks.append(
            f"[{listing.id}] {listing.title} — {price_str}\n{listing.description or '(no description)'}"
        )
    user = "\n\n".join(listing_blocks)

    return system, user
