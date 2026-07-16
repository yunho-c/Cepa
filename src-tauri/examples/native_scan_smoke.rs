use serde_json::Value;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tauri::{LogicalSize, Manager};

const STATE_FILENAME: &str = ".native-scan-smoke-window-state.json";
const REPORT_PREFIX: &str = "CEPA_NATIVE_SCAN_SMOKE:";
const SMOKE_TIMEOUT: Duration = Duration::from_secs(30);

fn main() {
    let process_started_at = Instant::now();
    let fixture = match std::env::args_os().nth(1).map(PathBuf::from) {
        Some(path) if path.is_dir() => path,
        Some(path) => {
            eprintln!("{} is not a directory", path.display());
            std::process::exit(2);
        }
        None => {
            eprintln!("usage: native_scan_smoke <directory>");
            std::process::exit(2);
        }
    };
    let fixture = fixture
        .canonicalize()
        .expect("canonicalize native scan smoke fixture");
    let fixture_json = serde_json::to_string(&fixture.to_string_lossy())
        .expect("serialize native scan smoke fixture path");
    let succeeded = Arc::new(AtomicBool::new(false));
    let state_path = Arc::new(Mutex::new(None));

    let app = cepa_lib::desktop_builder_for_smoke(STATE_FILENAME)
        .setup({
            let succeeded = Arc::clone(&succeeded);
            let state_path = Arc::clone(&state_path);
            move |app| {
                let isolated_state_path = app.path().app_config_dir()?.join(STATE_FILENAME);
                let _ = std::fs::remove_file(&isolated_state_path);
                *state_path.lock().expect("lock smoke state path") = Some(isolated_state_path);
                cepa_lib::initialize_desktop(app)?;
                let window = app.get_webview_window("main").ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "the main window was not created",
                    )
                })?;
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    if !cepa_lib::wait_for_desktop_page(&app_handle, SMOKE_TIMEOUT) {
                        eprintln!("the production frontend did not finish loading");
                        app_handle.exit(1);
                        return;
                    }
                    let page_ready_ms = process_started_at.elapsed().as_secs_f64() * 1_000.0;
                    if let Err(error) = window.set_size(LogicalSize::new(620.0, 480.0)) {
                        eprintln!(
                            "could not set the smoke window to the supported minimum: {error}"
                        );
                        app_handle.exit(1);
                        return;
                    }
                    let script = smoke_script(&fixture_json);
                    let deadline = Instant::now() + SMOKE_TIMEOUT;
                    let mut next_injection = Instant::now();
                    let mut injection_attempts = 0_u64;
                    let mut controller_started = false;
                    while Instant::now() < deadline {
                        match window.url() {
                            Ok(url)
                                if url.fragment().is_some_and(|fragment| {
                                    fragment.starts_with(REPORT_PREFIX)
                                }) =>
                            {
                                let fragment = url.fragment().expect("the report fragment exists");
                                let payload = &fragment[REPORT_PREFIX.len()..];
                                match decode_report(payload) {
                                    Ok(mut report) => {
                                        let object = report
                                            .as_object_mut()
                                            .expect("the smoke report is an object");
                                        object.insert(
                                            "injectionAttempts".into(),
                                            injection_attempts.into(),
                                        );
                                        object.insert(
                                            "pageReadyMs".into(),
                                            serde_json::json!(page_ready_ms),
                                        );
                                        object.insert(
                                            "processToReportMs".into(),
                                            serde_json::json!(
                                                process_started_at.elapsed().as_secs_f64()
                                                    * 1_000.0
                                            ),
                                        );
                                        println!(
                                            "{}",
                                            serde_json::to_string_pretty(&report)
                                                .expect("serialize native scan smoke report")
                                        );
                                        let passed = validate_report(&report);
                                        succeeded.store(passed, Ordering::Release);
                                        app_handle.exit(i32::from(!passed));
                                    }
                                    Err(error) => {
                                        eprintln!(
                                            "the WebView returned an invalid smoke report: {error}"
                                        );
                                        app_handle.exit(1);
                                    }
                                }
                                return;
                            }
                            Ok(url) => {
                                controller_started |= url.fragment().is_some_and(|fragment| {
                                    fragment.starts_with("CEPA_NATIVE_SCAN_SMOKE_PHASE:")
                                });
                                if !controller_started && Instant::now() >= next_injection {
                                    if let Err(error) = window.eval(&script) {
                                        eprintln!(
                                            "could not start the WebView smoke flow: {error}"
                                        );
                                        app_handle.exit(1);
                                        return;
                                    }
                                    injection_attempts += 1;
                                    next_injection =
                                        Instant::now() + Duration::from_millis(100);
                                }
                                std::thread::sleep(Duration::from_millis(10));
                            }
                            Err(error) => {
                                eprintln!("could not inspect the smoke WebView URL: {error}");
                                app_handle.exit(1);
                                return;
                            }
                        }
                    }
                    eprintln!(
                        "the native scan smoke flow timed out after {injection_attempts} injections at {}",
                        window
                            .url()
                            .map(|url| url.to_string())
                            .unwrap_or_else(|error| format!("an unreadable URL ({error})"))
                    );
                    app_handle.exit(1);
                });
                Ok(())
            }
        })
        .build(tauri::generate_context!())
        .expect("build native scan smoke application");

    let exit_code = app.run_return(|_, _| {});
    if let Some(path) = state_path.lock().expect("lock smoke state path").take() {
        let _ = std::fs::remove_file(path);
    }
    if exit_code != 0 || !succeeded.load(Ordering::Acquire) {
        std::process::exit(1);
    }
}

fn decode_report(encoded: &str) -> Result<Value, String> {
    if !encoded.len().is_multiple_of(2) {
        return Err("the hex payload has an odd length".into());
    }
    let mut bytes = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair)
            .map_err(|error| format!("the hex payload is not UTF-8: {error}"))?;
        bytes.push(
            u8::from_str_radix(pair, 16)
                .map_err(|error| format!("the report contains invalid hex: {error}"))?,
        );
    }
    let json = String::from_utf8(bytes)
        .map_err(|error| format!("the report JSON is not valid UTF-8: {error}"))?;
    serde_json::from_str(&json).map_err(|error| format!("the report JSON is invalid: {error}"))
}

fn validate_report(report: &Value) -> bool {
    let passed = report["ok"].as_bool() == Some(true)
        && report["pageReadyMs"]
            .as_f64()
            .is_some_and(|value| value > 0.0)
        && report["processToReportMs"]
            .as_f64()
            .is_some_and(|value| value > 0.0)
        && report["injectionAttempts"]
            .as_u64()
            .is_some_and(|count| count > 0)
        && report["initialRows"]
            .as_u64()
            .is_some_and(|count| count > 0)
        && report["chartSegments"]
            .as_u64()
            .is_some_and(|count| count > 0 && count <= 512)
        && report["chartTabStops"].as_u64() == Some(1)
        && report["logicalChartSegments"]
            .as_u64()
            .is_some_and(|count| count > 0 && count <= 512)
        && report["logicalChartTabStops"].as_u64() == Some(1)
        && report["listTabStops"]
            .as_u64()
            .is_some_and(|count| count <= 2)
        && report["horizontalOverflow"].as_bool() == Some(false)
        && report["pageErrors"].as_array().is_some_and(Vec::is_empty)
        && report["logicalMetricSelected"].as_bool() == Some(true)
        && report["navigationChangedFolder"].as_bool() == Some(true)
        && report["searchStatus"]
            .as_str()
            .is_some_and(|status| status.contains("match"))
        && report["searchedRows"]
            .as_u64()
            .is_some_and(|count| count > 0)
        && report["scanDetailsPresent"].as_bool() == Some(true)
        && report["backendLabel"]
            .as_str()
            .is_some_and(|label| !label.is_empty());
    if !passed {
        eprintln!("the native scan smoke report failed one or more invariants");
    }
    passed
}

fn smoke_script(fixture_json: &str) -> String {
    format!(
        r#"
if (!window.__CEPA_NATIVE_SCAN_SMOKE_STARTED__) {{
window.__CEPA_NATIVE_SCAN_SMOKE_STARTED__ = true;
void (async () => {{
  const fixture = {fixture_json};
  const pageErrors = [];
  window.addEventListener('error', (event) => pageErrors.push(String(event.error || event.message)));
  window.addEventListener('unhandledrejection', (event) => pageErrors.push(String(event.reason)));
  const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
  const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
  const painted = async () => {{ await frame(); await frame(); }};
  const waitFor = async (predicate, label, timeout = 30000) => {{
    const deadline = performance.now() + timeout;
    while (performance.now() < deadline) {{
      const value = predicate();
      if (value) return value;
      await sleep(10);
    }}
    throw new Error(`Timed out waiting for ${{label}}.`);
  }};
  const report = (payload) => {{
    const json = JSON.stringify(payload);
    const hex = [...new TextEncoder().encode(json)]
      .map((byte) => byte.toString(16).padStart(2, '0'))
      .join('');
    history.replaceState(null, '', '#{REPORT_PREFIX}' + hex);
  }};
  const phase = (name) => {{
    history.replaceState(null, '', '#CEPA_NATIVE_SCAN_SMOKE_PHASE:' + name);
  }};
  try {{
    phase('waiting-for-manual-path');
    const details = await waitFor(() => document.querySelector('.manual-path'), 'manual path entry');
    details.open = true;
    const input = details.querySelector('.path-input');
    const form = details.querySelector('.path-form');
    input.value = fixture;
    input.dispatchEvent(new Event('input', {{ bubbles: true }}));
    await frame();

    const scanStartedAt = performance.now();
    phase('scanning');
    form.requestSubmit();
    await waitFor(() => {{
      const error = document.querySelector('.error-callout');
      if (error) throw new Error(error.textContent?.trim() || 'The scan failed.');
      return document.querySelector('.results-view');
    }}, 'completed scan');
    await painted();
    phase('result-painted');
    const scanToPaintMs = performance.now() - scanStartedAt;
    const rootHeading = document.querySelector('.section-heading h2')?.textContent?.trim() || '';
    const initialRows = document.querySelectorAll('.storage-row').length;
    const chartSegments = document.querySelectorAll('[data-chart-node-id]').length;
    const chartTabStops = document.querySelectorAll('[data-chart-node-id][tabindex="0"]').length;
    const listTabStops = document.querySelectorAll('.storage-item[tabindex="0"], .reveal-item[tabindex="0"]').length;
    const horizontalOverflow = document.documentElement.scrollWidth > document.documentElement.clientWidth;
    const scanDetailsPresent = document.querySelector('.scan-details') !== null;
    const backendLabel = [...document.querySelectorAll('.scan-details dl > div')]
      .find((row) => row.querySelector('dt')?.textContent?.trim() === 'Scanner')
      ?.querySelector('dd')?.textContent?.trim() || '';

    const logicalButton = [...document.querySelectorAll('.metric-switch button')]
      .find((button) => button.textContent?.trim() === 'Logical');
    const logicalMetricSelected = () => [...document.querySelectorAll('.metric-switch button')]
      .find((button) => button.textContent?.trim() === 'Logical')
      ?.getAttribute('aria-pressed') === 'true';
    const metricStartedAt = performance.now();
    phase('switching-metric');
    logicalButton.click();
    await waitFor(() => logicalMetricSelected(), 'logical metric', 5000);
    await painted();
    const metricSwitchMs = performance.now() - metricStartedAt;
    const logicalChartSegments = document.querySelectorAll('[data-chart-node-id]').length;
    const logicalChartTabStops = document.querySelectorAll('[data-chart-node-id][tabindex="0"]').length;

    const firstDirectory = [...document.querySelectorAll('.storage-item')]
      .find((button) => button.querySelector('[data-kind="directory"]'));
    const navigationStartedAt = performance.now();
    phase('navigating');
    firstDirectory.click();
    await waitFor(
      () => document.querySelector('.section-heading h2')?.textContent?.trim() !== rootHeading,
      'directory navigation',
    );
    await painted();
    const navigationMs = performance.now() - navigationStartedAt;
    const navigatedHeading = document.querySelector('.section-heading h2')?.textContent?.trim() || '';
    const searchQuery = document.querySelector('.storage-item .item-copy > strong')?.textContent?.trim() || '';
    if (!searchQuery) throw new Error('The opened folder has no searchable rows.');

    const searchButton = document.querySelector('[aria-label="Search this folder"]');
    searchButton.click();
    const searchInput = await waitFor(
      () => document.querySelector('input[aria-label^="Find in "]'),
      'folder search input',
    );
    const searchStartedAt = performance.now();
    phase('searching');
    searchInput.value = searchQuery;
    searchInput.dispatchEvent(new Event('input', {{ bubbles: true }}));
    const searchStatus = await waitFor(() => {{
      const status = document.querySelector('#directory-search-status')?.textContent?.trim() || '';
      return status.includes('match') ? status : '';
    }}, 'folder search result');
    await painted();
    const searchMs = performance.now() - searchStartedAt;

    report({{
      ok: true,
      scanToPaintMs,
      metricSwitchMs,
      navigationMs,
      searchMs,
      initialRows,
      chartSegments,
      chartTabStops,
      logicalChartSegments,
      logicalChartTabStops,
      listTabStops,
      horizontalOverflow,
      logicalMetricSelected: logicalMetricSelected(),
      navigationChangedFolder: navigatedHeading !== rootHeading,
      searchStatus,
      searchedRows: document.querySelectorAll('.storage-row').length,
      scanDetailsPresent,
      backendLabel,
      pageErrors,
    }});
  }} catch (error) {{
    report({{
      ok: false,
      error: String(error),
      phase: location.hash,
      pageErrors,
      navigationError: document.querySelector('.navigation-error')?.textContent?.trim() || '',
      metricButtons: [...document.querySelectorAll('.metric-switch button')].map((button) => ({{
        label: button.textContent?.trim() || '',
        pressed: button.getAttribute('aria-pressed'),
        disabled: button.getAttribute('aria-disabled'),
      }})),
    }});
  }}
}})();
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::{decode_report, validate_report};

    #[test]
    fn decodes_hex_encoded_webview_reports() {
        let report = decode_report("7b226f6b223a747275657d").expect("decode report");
        assert_eq!(report["ok"], true);
        assert!(decode_report("0").is_err());
        assert!(decode_report("zz").is_err());
    }

    #[test]
    fn requires_every_native_flow_invariant() {
        let complete = serde_json::json!({
            "ok": true,
            "pageReadyMs": 100.0,
            "processToReportMs": 500.0,
            "injectionAttempts": 1,
            "initialRows": 4,
            "chartSegments": 8,
            "chartTabStops": 1,
            "logicalChartSegments": 8,
            "logicalChartTabStops": 1,
            "listTabStops": 2,
            "horizontalOverflow": false,
            "pageErrors": [],
            "logicalMetricSelected": true,
            "navigationChangedFolder": true,
            "searchStatus": "10 matches",
            "searchedRows": 10,
            "scanDetailsPresent": true,
            "backendLabel": "macOS native",
        });
        assert!(validate_report(&complete));

        let mut overflow = complete;
        overflow["horizontalOverflow"] = true.into();
        assert!(!validate_report(&overflow));
    }
}
