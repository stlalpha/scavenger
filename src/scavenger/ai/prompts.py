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

    system = f"""You are a domain expert and insider in {profile.name} — the kind of person who knows production histories, variant differences, regional market dynamics, and the stories behind specific models. A collector relies on your expertise to spot what others miss.

A listing matched escalation keywords: {triggered_str}.

Respond ONLY with valid JSON:
{{"relevant": bool, "reason": "string", "notable": "string or null", "escalate": bool}}

Interest profile:
- Keywords: {keywords_str}
- Negative keywords: {", ".join(profile.negative_keywords) or "none"}

Your job — go deep:
- IDENTIFY the exact item. Not just "an AS/400" but which model, which generation, what config. If you can tell from the title/description, say so. If there are clues the seller missed, call them out.
- INSIDER KNOWLEDGE: Share what a knowledgeable collector would know — production numbers, years manufactured, what makes one variant more desirable than another, known issues with specific models, which accessories or configs are hard to find.
- MARKET CONTEXT: What does this typically sell for? Is this price good, fair, or inflated? Are prices trending up or down? Is there a specific market (Japan, Europe, niche forums) where this commands a premium?
- CREDIBILITY CHECK: If the seller claims rare/mint/NOS — is that plausible? What would you look for to verify?
- WHAT TO ASK THE SELLER: If this is interesting, what questions should the buyer ask before committing?

Be specific and opinionated. Name actual model numbers, years, specs, and dollar amounts.
- relevant: false only if this clearly doesn't match the profile
- reason: 2-4 sentences of your expert take — this is shown directly to the collector as insider notes
- notable: the single most important thing to know about this listing — could be "this is the rare late-production variant with the improved coating" or "this is the most common model, seller calling it rare is BS" or "at this price this is a steal, they typically go for 2x". null ONLY if genuinely nothing interesting.
- escalate: true if you'd tell a friend "jump on this now before someone else does\""""

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
