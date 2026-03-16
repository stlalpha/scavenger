import pytest
import respx
import httpx
from pathlib import Path
from scavenger.tui.widgets.thumbnail import ThumbnailCache, PLACEHOLDER
from scavenger.dedup import content_hash


async def test_placeholder_for_no_url(tmp_path):
    cache = ThumbnailCache(cache_dir=tmp_path)
    result = await cache.get(None)
    assert result == PLACEHOLDER


@respx.mock
async def test_placeholder_on_download_failure(tmp_path):
    respx.get("https://example.com/img.jpg").mock(return_value=httpx.Response(404))
    cache = ThumbnailCache(cache_dir=tmp_path)
    result = await cache.get("https://example.com/img.jpg")
    assert result == PLACEHOLDER


async def test_cache_hit_skips_download(tmp_path):
    url = "https://example.com/real.jpg"
    cache_path = tmp_path / f"{content_hash(url)}.jpg"
    cache_path.write_bytes(b"FAKEJPEG")
    cache = ThumbnailCache(cache_dir=tmp_path)
    result = await cache.get(url)
    assert result == cache_path


@respx.mock
async def test_successful_download_cached(tmp_path):
    url = "https://example.com/new.jpg"
    respx.get(url).mock(return_value=httpx.Response(200, content=b"JPEGDATA"))
    cache = ThumbnailCache(cache_dir=tmp_path)
    result = await cache.get(url)
    assert isinstance(result, Path)
    assert result.exists()
    assert result.read_bytes() == b"JPEGDATA"


@respx.mock
async def test_timeout_returns_placeholder(tmp_path):
    respx.get("https://example.com/slow.jpg").mock(
        side_effect=httpx.TimeoutException("timeout")
    )
    cache = ThumbnailCache(cache_dir=tmp_path)
    result = await cache.get("https://example.com/slow.jpg")
    assert result == PLACEHOLDER
