//! Synthetic profiles and settings; no local game data or active LAN listener.
use bo2_server::{deployment::Deployment, profile::PublisherAssets};
use std::{fs, path::PathBuf, process::Command};
mod common;

fn temp() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "league_deployment_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    root
}

#[test]
fn local_profile_controls_bundle_requests_and_rejects_invalid_policy() {
    let mut profile = common::profile();
    profile.bundle_prefix = "local_profile_".into();
    let assets = PublisherAssets::from_profile(&profile, 5).unwrap();
    assert!(assets.is_bundle_name("local_profile_english.bundle"));
    assert!(!assets.is_bundle_name("example_online_english.bundle"));
    assert!(!PublisherAssets::default().is_bundle_name("anything"));
    profile.leagues[0].name = "bad\"label".into();
    assert!(profile.validate().is_err());
    let mut profile = common::profile();
    profile.leagues[1].id = profile.leagues[0].id;
    assert!(profile.validate().is_err());
    let mut profile = common::profile();
    profile.playlists[0].id = profile.leagues[0].playlist_id;
    assert!(profile.validate().is_err());
    let mut profile = common::profile();
    profile.maps[0] = "map\nsettings".into();
    assert!(profile.validate().is_err());
    let mut profile = common::profile();
    profile.playlists[0].category = "missing_category".into();
    assert!(profile.validate().is_err());
    let mut profile = common::profile();
    profile.league_member_name = profile.playlist_member_name.clone();
    assert!(profile.validate().is_err());
}

fn settings() -> toml::Table {
    toml::from_str(
        r#"
        # Paths are relative to this configuration file.
        bind = "127.0.0.1:3074"
        database = 'state/league.sqlite'
        profile = 'profile.json'
        experimental_bootstrap = true
        "#,
    )
    .unwrap()
}

#[test]
fn config_check_is_relative_to_its_file_and_does_not_create_database() {
    let root = temp();
    let path = root.join("settings.toml");
    fs::write(&path, toml::to_string(&settings()).unwrap()).unwrap();
    fs::write(
        root.join("profile.json"),
        include_str!("fixtures/profile.json"),
    )
    .unwrap();
    let config = Deployment::load(&path).unwrap();
    assert_eq!(
        config.profile.unwrap(),
        root.canonicalize().unwrap().join("profile.json")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_bo2-server"))
        .args(["--config", path.to_str().unwrap(), "--check-config"])
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("config.valid"));
    assert!(!root.join("state").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn config_requires_explicit_bootstrap_bind_and_real_profile() {
    let root = temp();
    let path = root.join("settings.toml");
    for invalid in [
        ("experimental_bootstrap", toml::Value::Boolean(false)),
        ("allow_ips", toml::Value::Array(vec![])),
        ("bind", toml::Value::String("0.0.0.0:3074".into())),
        ("profile", toml::Value::String("".into())),
        ("unexpected", toml::Value::Boolean(true)),
    ] {
        let mut config = settings();
        config.insert(invalid.0.into(), invalid.1);
        fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        assert!(Deployment::load(&path).is_err());
    }
    fs::write(&path, toml::to_string(&settings()).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bo2-server"))
        .args(["--config", path.to_str().unwrap(), "--check-config"])
        .output()
        .unwrap();
    assert!(!output.status.success()); // Missing profile must never fall back.
    assert!(!root.join("state").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_or_legacy_settings_are_rejected_and_example_is_valid_toml() {
    let root = temp();
    let path = root.join("settings.toml");
    let valid = toml::to_string(&settings()).unwrap();
    for invalid in [
        format!("{valid}\nbind = \"127.0.0.1:3074\""),
        valid.replace(
            "experimental_bootstrap = true",
            "experimental_bootstrap = 'true'",
        ),
        "bind = [".into(),
        serde_json::to_string(&settings()).unwrap(),
    ] {
        fs::write(&path, invalid).unwrap();
        assert!(Deployment::load(&path).is_err());
    }
    let mut example: toml::Table =
        toml::from_str(include_str!("../bo2-server.example.toml")).unwrap();
    assert_eq!(example["experimental_bootstrap"].as_bool(), Some(false));
    example.insert("experimental_bootstrap".into(), toml::Value::Boolean(true));
    fs::write(&path, toml::to_string(&example).unwrap()).unwrap();
    assert!(Deployment::load(&path).is_ok());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn executable_mode_is_explicit_relative_and_rejects_unknown_inputs_without_output() {
    let root = temp();
    let path = root.join("settings.toml");
    let mut config = settings();
    config.insert("executable".into(), "input.elf".into());
    fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    assert!(Deployment::load(&path).is_err()); // Never silently prefer one source.
    config.remove("profile");
    fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    assert_eq!(
        Deployment::load(&path).unwrap().executable.unwrap(),
        root.canonicalize().unwrap().join("input.elf")
    );
    fs::write(root.join("input.elf"), b"unsupported synthetic executable").unwrap();
    for option in ["--write-bundle", "--write-profile", "--write-endpoints"] {
        let output = root.join("derived");
        let result = Command::new(env!("CARGO_BIN_EXE_bo2-server"))
            .args([
                "--config",
                path.to_str().unwrap(),
                option,
                output.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("unsupported executable"));
        assert!(!output.exists());
        assert!(!root.join("state").exists());
    }
    config.remove("executable");
    fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    assert!(Deployment::load(&path).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_exports_preserve_existing_files_and_do_not_guess_endpoints_from_profile() {
    let root = temp();
    let path = root.join("settings.toml");
    fs::write(&path, toml::to_string(&settings()).unwrap()).unwrap();
    let profile = root.join("profile.json");
    let original = include_bytes!("fixtures/profile.json");
    fs::write(&profile, original).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_bo2-server"))
        .args([
            "--config",
            path.to_str().unwrap(),
            "--write-bundle",
            profile.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read(&profile).unwrap(), original);
    for option in ["--write-profile", "--write-endpoints"] {
        let output = root.join("derived.json");
        let result = Command::new(env!("CARGO_BIN_EXE_bo2-server"))
            .args([
                "--config",
                path.to_str().unwrap(),
                option,
                output.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!output.exists());
    }
    assert!(!root.join("state").exists());
    fs::remove_dir_all(root).unwrap();
}
