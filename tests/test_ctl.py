from click.testing import CliRunner
from scavenger.ctl import cli


def test_help():
    result = CliRunner().invoke(cli, ["--help"])
    assert result.exit_code == 0

def test_list_profiles_missing_config(tmp_path):
    result = CliRunner().invoke(cli, ["--config", str(tmp_path / "nope.toml"), "list-profiles"])
    assert result.exit_code != 0
    assert "error" in result.output.lower() or "not found" in result.output.lower()

def test_list_profiles_with_config(tmp_path):
    cfg = tmp_path / "config.toml"
    cfg.write_text(
        '[[profiles]]\nid = "sony"\nname = "Sony Glass"\n'
        'keywords = ["sony"]\nnegative_keywords = []\nsources = ["ebay"]\nenabled = true\n'
    )
    result = CliRunner().invoke(cli, ["--config", str(cfg), "list-profiles"])
    assert result.exit_code == 0
    assert "Sony Glass" in result.output
