from scavenger.dedup import normalize_url, content_hash

def test_normalize_strips_utm_params():
    url = "https://www.ebay.com/itm/123?utm_source=newsletter&utm_medium=email"
    assert normalize_url(url) == "https://www.ebay.com/itm/123"

def test_normalize_strips_ebay_tracking():
    url = "https://www.ebay.com/itm/123?ssPageName=STRK&_trkparms=aid%3D111001"
    assert normalize_url(url) == "https://www.ebay.com/itm/123"

def test_normalize_removes_fragment():
    url = "https://ebay.com/itm/123#description"
    assert "#" not in normalize_url(url)

def test_content_hash_is_stable():
    url = "https://www.ebay.com/itm/123456789"
    assert content_hash(url) == content_hash(url)

def test_content_hash_differs_for_different_urls():
    assert content_hash("https://ebay.com/itm/111") != content_hash("https://ebay.com/itm/222")

def test_content_hash_is_64_char_hex():
    h = content_hash("https://example.com/item/1")
    assert len(h) == 64
    assert all(c in "0123456789abcdef" for c in h)
