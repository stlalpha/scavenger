//! SOPS-exclusive secret resolution. Runs as its own test binary/process
//! so PATH/env mutation cannot race other tests; scenarios run inside one
//! test fn to serialize the env changes.

use scavenger::ai::models::AIConfig;
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn write_shim(dir: &std::path::Path, script: &str) {
    let path = dir.join("sops");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn resolve_with_env(vars: &[(&str, Option<&str>)]) -> String {
    for (k, v) in vars {
        match v {
            Some(val) => std::env::set_var(k, val),
            None => std::env::remove_var(k),
        }
    }
    let mut cfg = AIConfig::default();
    cfg.resolve_env_keys();
    cfg.anthropic_api_key
}

#[test]
fn sops_exclusive_key_resolution() {
    let tmp = std::env::temp_dir().join(format!("scav-sops-test-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    let secrets = tmp.join("secrets.sops.yaml");
    fs::write(&secrets, "anthropic_api_key: ENC[placeholder]\n").unwrap();
    let orig_path = std::env::var("PATH").unwrap();
    let shim_path = format!("{}:{orig_path}", tmp.display());

    // 1. Key comes from sops decryption of the secrets file.
    write_shim(&tmp, "#!/bin/sh\necho sk-from-sops\n");
    std::env::set_var("PATH", &shim_path);
    let key = resolve_with_env(&[
        ("ANTHROPIC_API_KEY", None),
        ("SCAVENGER_SECRETS_FILE", Some(secrets.to_str().unwrap())),
    ]);
    assert_eq!(key, "sk-from-sops");

    // 2. Environment variable (sops exec-env style) takes precedence and
    //    the sops binary is not consulted at all.
    write_shim(&tmp, "#!/bin/sh\necho SHOULD-NOT-RUN; exit 1\n");
    let key = resolve_with_env(&[("ANTHROPIC_API_KEY", Some("sk-from-env"))]);
    assert_eq!(key, "sk-from-env");

    // 3. Missing secrets file -> no key (surfaced later via AI health).
    let key = resolve_with_env(&[
        ("ANTHROPIC_API_KEY", None),
        ("SCAVENGER_SECRETS_FILE", Some("/nonexistent/secrets.sops.yaml")),
    ]);
    assert_eq!(key, "");

    // 4. sops failure -> no key, not a crash.
    write_shim(&tmp, "#!/bin/sh\necho decrypt boom >&2; exit 1\n");
    let key = resolve_with_env(&[
        ("ANTHROPIC_API_KEY", None),
        ("SCAVENGER_SECRETS_FILE", Some(secrets.to_str().unwrap())),
    ]);
    assert_eq!(key, "");

    // 5. sops returning an empty value -> treated as missing.
    write_shim(&tmp, "#!/bin/sh\necho\n");
    let key = resolve_with_env(&[
        ("ANTHROPIC_API_KEY", None),
        ("SCAVENGER_SECRETS_FILE", Some(secrets.to_str().unwrap())),
    ]);
    assert_eq!(key, "");

    std::env::set_var("PATH", orig_path);
    std::env::remove_var("SCAVENGER_SECRETS_FILE");
    fs::remove_dir_all(&tmp).ok();
}
