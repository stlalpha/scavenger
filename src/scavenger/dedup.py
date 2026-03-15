import hashlib
from urllib.parse import urlparse, urlencode, parse_qsl, urlunparse

STRIP_PARAMS = frozenset({
    "utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content",
    "ssPageName", "_trkparms", "_trktoken", "hash", "ref",
    "mkevt", "mkcid", "mkrid", "campid", "toolid",
})


def normalize_url(url: str) -> str:
    parsed = urlparse(url)
    filtered = [
        (k, v) for k, v in parse_qsl(parsed.query)
        if k not in STRIP_PARAMS and not k.startswith("utm_")
    ]
    return urlunparse(parsed._replace(query=urlencode(filtered), fragment=""))


def content_hash(url: str) -> str:
    """Return a stable SHA-256 hex digest of the normalized URL.

    Used as a listing's primary key. Hashes the URL (not content)
    so the same listing at the same URL is always the same ID.
    """
    return hashlib.sha256(normalize_url(url).encode()).hexdigest()
