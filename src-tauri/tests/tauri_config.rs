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
