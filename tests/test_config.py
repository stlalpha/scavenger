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

def test_missing_id_raises():
    with pytest.raises(ConfigError):
        load_config(FIXTURES / "invalid_missing_id.toml")

def test_bad_priority_raises():
    with pytest.raises(ConfigError):
        load_config(FIXTURES / "invalid_bad_priority.toml")

def test_socket_path_is_expanded():
    config = load_config(FIXTURES / "valid_config.toml")
    assert not str(config.socket_path).startswith("~")
    assert config.socket_path.is_absolute()

def test_global_defaults_when_no_global_section(tmp_path):
    cfg = tmp_path / "minimal.toml"
    cfg.write_text('[[profiles]]\nid = "p1"\nname = "Test"\nkeywords = ["test"]\nnegative_keywords = []\nsources = ["ebay"]\n')
    config = load_config(cfg)
    assert "scavenger" in str(config.db_path)
    assert config.log_level == "INFO"
