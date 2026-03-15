import pytest
from pathlib import Path
from scavenger.config import load_config, ConfigError

FIXTURES = Path(__file__).parent / "fixtures"

def test_load_valid_config():
    config = load_config(FIXTURES / "valid_config.toml")
    assert len(config.profiles) == 2
    assert config.profiles[0].name == "Sony A-mount Glass"
    assert config.profiles[0].price_max == 800.0

def test_profile_keywords_preserved():
    config = load_config(FIXTURES / "valid_config.toml")
    profile = config.profiles[0]
    assert "sony" in profile.keywords
    assert ["a-mount", "alpha mount"] in profile.keywords

def test_load_invalid_config_raises():
    with pytest.raises(ConfigError):
        load_config(FIXTURES / "invalid_config.toml")

def test_missing_file_raises():
    with pytest.raises(ConfigError):
        load_config(Path("/nonexistent/config.toml"))

def test_db_path_is_expanded():
    config = load_config(FIXTURES / "valid_config.toml")
    assert not str(config.db_path).startswith("~")
