use serde_json::{Map, Value};

fn security_config() -> Map<String, Value> {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .expect("tauri.conf.json must be valid JSON");
    config["app"]["security"]
        .as_object()
        .expect("app.security must be an object")
        .clone()
}

fn directive_sources<'a>(policy: &'a Value, directive: &str) -> Vec<&'a str> {
    policy[directive]
        .as_array()
        .unwrap_or_else(|| panic!("{directive} must be an array"))
        .iter()
        .map(|source| {
            source
                .as_str()
                .unwrap_or_else(|| panic!("{directive} sources must be strings"))
        })
        .collect()
}

#[test]
fn packaged_webview_policy_allows_only_bundled_assets_and_tauri_ipc() {
    let security = security_config();
    let policy = &security["csp"];

    assert_eq!(directive_sources(policy, "default-src"), ["'self'"]);
    assert_eq!(
        directive_sources(policy, "connect-src"),
        ["ipc:", "http://ipc.localhost"]
    );
    assert_eq!(directive_sources(policy, "img-src"), ["'self'"]);
    assert_eq!(directive_sources(policy, "script-src"), ["'self'"]);
    assert_eq!(directive_sources(policy, "style-src"), ["'self'"]);

    for directive in [
        "object-src",
        "base-uri",
        "frame-src",
        "frame-ancestors",
        "worker-src",
    ] {
        assert_eq!(directive_sources(policy, directive), ["'none'"]);
    }

    let serialized = serde_json::to_string(policy).expect("CSP must serialize");
    for forbidden in [
        "unsafe-inline",
        "unsafe-eval",
        "localhost:1420",
        "data:",
        "blob:",
        "https:",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "production CSP must not contain {forbidden}"
        );
    }
    assert_ne!(
        security.get("dangerousDisableAssetCspModification"),
        Some(&Value::Bool(true)),
        "Tauri must retain control of per-asset hashes and nonces"
    );
}

#[test]
fn development_policy_limits_relaxation_to_vite_styles_and_hmr() {
    let security = security_config();
    let policy = &security["devCsp"];

    assert_eq!(directive_sources(policy, "default-src"), ["'self'"]);
    assert_eq!(directive_sources(policy, "img-src"), ["'self'"]);
    assert_eq!(directive_sources(policy, "script-src"), ["'self'"]);
    assert_eq!(
        directive_sources(policy, "style-src"),
        ["'self'", "'unsafe-inline'"]
    );
    assert_eq!(
        directive_sources(policy, "connect-src"),
        [
            "'self'",
            "ipc:",
            "http://ipc.localhost",
            "ws://localhost:1420"
        ]
    );

    for directive in [
        "object-src",
        "base-uri",
        "frame-src",
        "frame-ancestors",
        "worker-src",
    ] {
        assert_eq!(directive_sources(policy, directive), ["'none'"]);
    }

    let serialized = serde_json::to_string(policy).expect("development CSP must serialize");
    assert!(!serialized.contains("unsafe-eval"));
    assert!(!serialized.contains("data:"));
    assert!(!serialized.contains("blob:"));
    assert!(!serialized.contains("https:"));
    assert!(!serialized.contains("wss:"));
}

#[test]
fn distribution_metadata_is_complete_and_consistent() {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .expect("tauri.conf.json must be valid JSON");
    let package: Value = serde_json::from_str(include_str!("../../package.json"))
        .expect("package.json must be valid JSON");
    let bundle = &config["bundle"];
    let repository = "https://github.com/yunho-c/Cepa";

    assert_eq!(config["version"], package["version"]);
    assert_eq!(config["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(package["private"], true);
    assert_eq!(package["license"], env!("CARGO_PKG_LICENSE"));
    assert_eq!(package["homepage"], env!("CARGO_PKG_HOMEPAGE"));
    assert_eq!(package["repository"], env!("CARGO_PKG_REPOSITORY"));
    assert_eq!(package["repository"], repository);

    let cargo_manifest = include_str!("../Cargo.toml");
    assert!(cargo_manifest.lines().any(|line| line == "publish = false"));
    assert!(
        cargo_manifest
            .lines()
            .any(|line| line == "authors = [\"Cepa contributors\"]")
    );

    assert_eq!(bundle["publisher"], "Cepa contributors");
    assert_eq!(bundle["homepage"], repository);
    assert_eq!(bundle["copyright"], "Copyright © 2026 Cepa contributors");
    assert_eq!(bundle["license"], "MIT");
    assert_eq!(bundle["licenseFile"], "../LICENSE");
    assert_eq!(bundle["category"], "Utility");
    assert!(
        bundle["shortDescription"]
            .as_str()
            .is_some_and(|description| !description.trim().is_empty())
    );
    assert!(
        bundle["longDescription"]
            .as_str()
            .is_some_and(|description| description.contains("stays on this device"))
    );

    let license = include_str!("../../LICENSE");
    assert!(license.starts_with("MIT License\n"));
    assert!(license.contains("Copyright (c) 2026 Cepa contributors"));
    assert!(license.contains("THE SOFTWARE IS PROVIDED \"AS IS\""));
}

#[test]
fn canonical_javascript_workflows_force_the_bun_runtime() {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .expect("tauri.conf.json must be valid JSON");
    assert_eq!(config["build"]["beforeDevCommand"], "bun --bun run dev");
    assert_eq!(config["build"]["beforeBuildCommand"], "bun --bun run build");

    let justfile = include_str!("../../Justfile");
    for command in [
        "bun --bun run desktop:dev",
        "bun --bun run dev",
        "bun --bun run check",
        "bun --bun run icons",
        "bun --bun run tauri build --no-bundle",
        "bun --bun run tauri build",
    ] {
        assert!(
            justfile.contains(command),
            "canonical Justfile must contain {command}"
        );
    }
}

#[test]
fn linux_ci_smokes_the_built_desktop_before_packaging() {
    let workflow = include_str!("../../.github/workflows/ci.yml");

    for dependency in ["dbus-daemon", "xvfb"] {
        assert!(
            workflow
                .lines()
                .any(|line| line.trim() == format!("{dependency} \\")),
            "Linux CI must install {dependency} explicitly"
        );
    }

    let build = workflow
        .find("- name: Build native application")
        .expect("CI must build the native application");
    let smoke = workflow
        .find("- name: Smoke Linux desktop startup")
        .expect("CI must smoke-test the Linux desktop executable");
    let bundle = workflow
        .find("- name: Build distribution bundles")
        .expect("CI must build distribution bundles");
    let package_validation = workflow
        .find("- name: Validate, smoke, and archive Linux distribution bundles")
        .expect("CI must validate and smoke-test Linux distribution bundles");
    let macos_validation = workflow
        .find("- name: Validate and archive macOS distribution bundles")
        .expect("CI must validate macOS distribution bundles");
    assert!(build < smoke && smoke < bundle && bundle < package_validation);

    let smoke_step = &workflow[smoke..bundle];
    assert!(smoke_step.contains("if: runner.os == 'Linux'"));
    assert!(smoke_step.contains("run: just smoke-linux-desktop"));

    let package_step = &workflow[package_validation..macos_validation];
    assert!(package_step.contains("just validate-linux-bundles"));
    assert!(package_step.contains("-name '*.AppImage' -print -quit"));
    assert!(
        package_step.contains("APPIMAGE_EXTRACT_AND_RUN=1 just smoke-linux-desktop \"$appimage\"")
    );
}
