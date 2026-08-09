"""SOPS-exclusive secret resolution for AIConfig."""

import os
import stat

from scavenger.ai.models import AIConfig


def _write_shim(tmp_path, script: str) -> None:
    shim = tmp_path / "sops"
    shim.write_text(script)
    shim.chmod(shim.stat().st_mode | stat.S_IEXEC)


def _isolate(monkeypatch, tmp_path, secrets_file) -> None:
    monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
    monkeypatch.setenv("SCAVENGER_SECRETS_FILE", str(secrets_file))
    monkeypatch.setenv("PATH", f"{tmp_path}:{os.environ['PATH']}")


def test_key_resolves_via_sops(monkeypatch, tmp_path):
    secrets = tmp_path / "secrets.sops.yaml"
    secrets.write_text("anthropic_api_key: ENC[placeholder]\n")
    _write_shim(tmp_path, "#!/bin/sh\necho sk-from-sops\n")
    _isolate(monkeypatch, tmp_path, secrets)

    assert AIConfig().anthropic_api_key == "sk-from-sops"


def test_env_var_takes_precedence_over_sops(monkeypatch, tmp_path):
    secrets = tmp_path / "secrets.sops.yaml"
    secrets.write_text("anthropic_api_key: ENC[placeholder]\n")
    _write_shim(tmp_path, "#!/bin/sh\necho SHOULD-NOT-RUN; exit 1\n")
    _isolate(monkeypatch, tmp_path, secrets)
    monkeypatch.setenv("ANTHROPIC_API_KEY", "sk-from-env")

    assert AIConfig().anthropic_api_key == "sk-from-env"


def test_missing_secrets_file_yields_no_key(monkeypatch, tmp_path):
    _isolate(monkeypatch, tmp_path, tmp_path / "does-not-exist.sops.yaml")

    assert AIConfig().anthropic_api_key == ""


def test_sops_failure_yields_no_key_not_crash(monkeypatch, tmp_path):
    secrets = tmp_path / "secrets.sops.yaml"
    secrets.write_text("anthropic_api_key: ENC[placeholder]\n")
    _write_shim(tmp_path, "#!/bin/sh\necho decrypt boom >&2; exit 1\n")
    _isolate(monkeypatch, tmp_path, secrets)

    assert AIConfig().anthropic_api_key == ""


def test_empty_sops_value_treated_as_missing(monkeypatch, tmp_path):
    secrets = tmp_path / "secrets.sops.yaml"
    secrets.write_text('anthropic_api_key: ""\n')
    _write_shim(tmp_path, "#!/bin/sh\necho\n")
    _isolate(monkeypatch, tmp_path, secrets)

    assert AIConfig().anthropic_api_key == ""
