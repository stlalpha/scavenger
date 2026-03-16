import pytest
from pathlib import Path
from scavenger.config import load_ai_config, ConfigError
from scavenger.ai.models import AIConfig

FIXTURES = Path(__file__).parent / "fixtures"


def test_load_ai_config_from_toml():
    config = load_ai_config(FIXTURES / "ai_config.toml")
    assert config.enabled is True
    assert config.filter_model == "qwen3.5:9b"


def test_load_ai_config_missing_file_returns_disabled():
    config = load_ai_config(Path("/nonexistent/ai.toml"))
    assert config.enabled is False


def test_load_ai_config_invalid_toml_raises():
    import tempfile, os
    with tempfile.NamedTemporaryFile(mode='w', suffix='.toml', delete=False) as f:
        f.write("this is not [valid toml")
        tmp = Path(f.name)
    try:
        with pytest.raises(ConfigError):
            load_ai_config(tmp)
    finally:
        os.unlink(tmp)


def test_daemon_uses_noop_when_ai_disabled():
    from scavenger.daemon.main import Daemon
    from scavenger.config import AppConfig, GlobalConfig
    from scavenger.ai.evaluator import NoopEvaluator
    config = AppConfig(
        global_config=GlobalConfig(db_path="/tmp/test.db", socket_path="/tmp/test.sock"),
        profiles=[],
    )
    daemon = Daemon(config, ai_config=None)
    assert isinstance(daemon._evaluator, NoopEvaluator)


def test_daemon_uses_ai_evaluator_when_enabled():
    from scavenger.daemon.main import Daemon
    from scavenger.config import AppConfig, GlobalConfig
    from scavenger.ai.evaluator import AIEvaluator
    config = AppConfig(
        global_config=GlobalConfig(db_path="/tmp/test.db", socket_path="/tmp/test.sock"),
        profiles=[],
    )
    ai_config = AIConfig(enabled=True)
    daemon = Daemon(config, ai_config=ai_config)
    assert isinstance(daemon._evaluator, AIEvaluator)
