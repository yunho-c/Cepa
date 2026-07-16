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
            eprintln!(
                "usage: native_scan_smoke <directory> [cancellation-directory] [failure-file]"
            );
            std::process::exit(2);
        }
    };
    let fixture = fixture
        .canonicalize()
        .expect("canonicalize native scan smoke fixture");
    let fixture_json = serde_json::to_string(&fixture.to_string_lossy())
        .expect("serialize native scan smoke fixture path");
    let cancellation_fixture = match std::env::args_os().nth(2).map(PathBuf::from) {
        Some(path) if path.is_dir() => Some(
            path.canonicalize()
                .expect("canonicalize native cancellation smoke fixture"),
        ),
        Some(path) => {
            eprintln!("{} is not a cancellation fixture directory", path.display());
            std::process::exit(2);
        }
        None => None,
    };
    let cancellation_fixture_json = serde_json::to_string(
        &cancellation_fixture
            .as_ref()
            .map(|path| path.to_string_lossy()),
    )
    .expect("serialize native cancellation smoke fixture path");
    let failure_fixture = match std::env::args_os().nth(3).map(PathBuf::from) {
        Some(path) if path.is_file() => Some(
            path.canonicalize()
                .expect("canonicalize native failure smoke fixture"),
        ),
        Some(path) => {
            eprintln!("{} is not a failure fixture file", path.display());
            std::process::exit(2);
        }
        None => None,
    };
    let failure_fixture_json =
        serde_json::to_string(&failure_fixture.as_ref().map(|path| path.to_string_lossy()))
            .expect("serialize native failure smoke fixture path");
    let completed_scan_id =
        1 + u64::from(cancellation_fixture.is_some()) + u64::from(failure_fixture.is_some());
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
                    let script = smoke_script(
                        &fixture_json,
                        &cancellation_fixture_json,
                        &failure_fixture_json,
                        completed_scan_id,
                    );
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
                                        let passed = validate_report(&report, completed_scan_id);
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

fn validate_report(report: &Value, expected_discarded_scan_id: u64) -> bool {
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
        && report["chartArrowMoved"].as_bool() == Some(true)
        && report["chartHomeMoved"].as_bool() == Some(true)
        && report["listTabStops"]
            .as_u64()
            .is_some_and(|count| count <= 2)
        && report["listArrowMoved"].as_bool() == Some(true)
        && report["revealArrowPreserved"].as_bool() == Some(true)
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
        && report["terminalFailureShown"].as_bool() == Some(true)
        && report["terminalFailureFocusRestored"].as_bool() == Some(true)
        && report["terminalFailureRecoveryAvailable"].as_bool() == Some(true)
        && report["cancellationStopped"].as_bool() == Some(true)
        && report["cancellationFocusRestored"].as_bool() == Some(true)
        && report["cancellationRecoveryAvailable"].as_bool() == Some(true)
        && report["cancellationMs"]
            .as_f64()
            .is_some_and(|value| value >= 0.0)
        && report["landingFocusRestored"].as_bool() == Some(true)
        && report["discardedScanId"].as_u64() == Some(expected_discarded_scan_id)
        && report["staleScanRejected"].as_bool() == Some(true)
        && report["rescanCompleted"].as_bool() == Some(true)
        && report["rescanResultFocused"].as_bool() == Some(true)
        && report["rescanRows"].as_u64().is_some_and(|count| count > 0)
        && report["rescanChartSegments"]
            .as_u64()
            .is_some_and(|count| count > 0 && count <= 512)
        && report["rescanBackendLabel"] == report["backendLabel"]
        && report["scanDetailsPresent"].as_bool() == Some(true)
        && report["backendLabel"]
            .as_str()
            .is_some_and(|label| !label.is_empty());
    if !passed {
        eprintln!("the native scan smoke report failed one or more invariants");
    }
    passed
}

fn smoke_script(
    fixture_json: &str,
    cancellation_fixture_json: &str,
    failure_fixture_json: &str,
    completed_scan_id: u64,
) -> String {
    format!(
        r#"
if (!window.__CEPA_NATIVE_SCAN_SMOKE_STARTED__) {{
window.__CEPA_NATIVE_SCAN_SMOKE_STARTED__ = true;
void (async () => {{
  const fixture = {fixture_json};
  const cancellationFixture = {cancellation_fixture_json};
  const failureFixture = {failure_fixture_json};
  const completedScanId = {completed_scan_id};
  const pageErrors = [];
  window.addEventListener('error', (event) => pageErrors.push(String(event.error || event.message)));
  window.addEventListener('unhandledrejection', (event) => pageErrors.push(String(event.reason)));
  const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
  const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
  const painted = async () => {{ await frame(); await frame(); }};
  const press = async (target, key) => {{
    target.dispatchEvent(new KeyboardEvent('keydown', {{ key, bubbles: true }}));
    await painted();
    return document.activeElement;
  }};
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
  const submitPath = async (path, label) => {{
    const details = await waitFor(() => document.querySelector('.manual-path'), `${{label}} manual path entry`);
    details.open = true;
    const input = details.querySelector('.path-input');
    const form = details.querySelector('.path-form');
    input.value = path;
    input.dispatchEvent(new Event('input', {{ bubbles: true }}));
    await frame();
    form.requestSubmit();
  }};
  const submitFixture = (label) => submitPath(fixture, label);
  const backendLabel = () => [...document.querySelectorAll('.scan-details dl > div')]
    .find((row) => row.querySelector('dt')?.textContent?.trim() === 'Scanner')
    ?.querySelector('dd')?.textContent?.trim() || '';
  try {{
    let terminalFailureShown = failureFixture === null;
    let terminalFailureFocusRestored = failureFixture === null;
    let terminalFailureRecoveryAvailable = failureFixture === null;
    if (failureFixture !== null) {{
      phase('failing-scan');
      await submitPath(failureFixture, 'failure');
      const failureNotice = await waitFor(
        () => document.querySelector('.landing .error-callout'),
        'terminal scan failure',
        10000,
      );
      await painted();
      terminalFailureShown =
        failureNotice.textContent?.includes('The scan couldn’t finish.') === true;
      terminalFailureFocusRestored = document.activeElement === failureNotice;
      terminalFailureRecoveryAvailable = document.querySelector('.manual-path') !== null;
    }}

    let cancellationStopped = cancellationFixture === null;
    let cancellationFocusRestored = cancellationFixture === null;
    let cancellationRecoveryAvailable = cancellationFixture === null;
    let cancellationMs = 0;
    if (cancellationFixture !== null) {{
      phase('cancelling-scan');
      const details = await waitFor(
        () => document.querySelector('.manual-path'),
        'cancellation manual path entry',
      );
      details.open = true;
      const input = details.querySelector('.path-input');
      input.value = cancellationFixture;
      input.dispatchEvent(new Event('input', {{ bubbles: true }}));
      await frame();
      const cancellationStartedAt = performance.now();
      details.querySelector('.path-form').requestSubmit();
      const stopButton = await waitFor(
        () => [...document.querySelectorAll('.scan-view button')]
          .find((button) => button.textContent?.trim() === 'Stop' && !button.disabled),
        'enabled Stop control',
        5000,
      );
      stopButton.click();
      const cancelledNotice = await waitFor(
        () => {{
          if (document.querySelector('.results-view')) {{
            throw new Error('The cancellation fixture completed before Stop took effect.');
          }}
          return document.querySelector('.cancelled-callout');
        }},
        'cancelled landing state',
        10000,
      );
      await painted();
      cancellationMs = performance.now() - cancellationStartedAt;
      cancellationStopped = cancelledNotice.textContent?.includes('Scan stopped.') === true;
      cancellationFocusRestored = document.activeElement === cancelledNotice;
      cancellationRecoveryAvailable = document.querySelector('.manual-path') !== null;
    }}

    phase('waiting-for-manual-path');
    const scanStartedAt = performance.now();
    phase('scanning');
    await submitFixture('initial');
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
    const initialBackendLabel = backendLabel();

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
    const logicalMetricWasSelected = logicalMetricSelected();

    const chartNodes = [...document.querySelectorAll('[data-chart-node-id]')];
    if (chartNodes.length < 2) throw new Error('The logical chart needs two keyboard targets.');
    chartNodes[0].focus();
    const chartArrowTarget = await press(chartNodes[0], 'ArrowRight');
    const chartArrowMoved = chartArrowTarget?.dataset.chartNodeId === chartNodes[1].dataset.chartNodeId;
    const chartHomeTarget = await press(chartArrowTarget, 'Home');
    const chartHomeMoved = chartHomeTarget?.dataset.chartNodeId === chartNodes[0].dataset.chartNodeId;

    const openButtons = [...document.querySelectorAll('.storage-item')];
    const revealButtons = [...document.querySelectorAll('.reveal-item')];
    if (openButtons.length < 2 || revealButtons.length < 2) {{
      throw new Error('The directory list needs two open and Reveal keyboard targets.');
    }}
    openButtons[0].focus();
    const listArrowTarget = await press(openButtons[0], 'ArrowDown');
    const listArrowMoved = listArrowTarget?.dataset.listOpenId === openButtons[1].dataset.listOpenId;
    revealButtons[0].focus();
    const revealArrowTarget = await press(revealButtons[0], 'ArrowDown');
    const revealArrowPreserved = revealArrowTarget?.dataset.listRevealId === revealButtons[1].dataset.listRevealId;

    const navigationStartedAt = performance.now();
    phase('navigating');
    chartNodes[0].focus();
    chartNodes[0].dispatchEvent(new KeyboardEvent('keydown', {{ key: 'Enter', bubbles: true }}));
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
    const searchedRows = document.querySelectorAll('.storage-row').length;

    phase('returning-home');
    document.querySelector('[aria-label="Cepa home"]').click();
    const landingHeading = await waitFor(
      () => document.querySelector('#landing-title'),
      'landing view after discard',
    );
    await painted();
    const landingFocusRestored = document.activeElement === landingHeading;
    let staleScanRejected = false;
    try {{
      // The smoke builder owns a fresh ScanState. When cancellation ran first,
      // Home must release scan 2 rather than merely leaving scan 1 stale.
      await window.__TAURI_INTERNALS__.invoke('open_scan_directory', {{
        scanId: completedScanId,
        nodeId: 0,
        metric: 'allocated',
      }});
    }} catch {{
      staleScanRejected = true;
    }}

    phase('rescanning');
    await submitFixture('rescan');
    await waitFor(() => document.querySelector('.results-view'), 'rescanned result');
    await painted();
    const rescanCompleted = document.querySelector('.results-view') !== null;
    const rescanHeading = document.querySelector('.result-title h1');
    const rescanResultFocused = document.activeElement === rescanHeading;
    const rescanRows = document.querySelectorAll('.storage-row').length;
    const rescanChartSegments = document.querySelectorAll('[data-chart-node-id]').length;
    const rescanBackendLabel = backendLabel();

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
      chartArrowMoved,
      chartHomeMoved,
      listTabStops,
      listArrowMoved,
      revealArrowPreserved,
      horizontalOverflow,
      logicalMetricSelected: logicalMetricWasSelected,
      navigationChangedFolder: navigatedHeading !== rootHeading,
      searchStatus,
      searchedRows,
      terminalFailureShown,
      terminalFailureFocusRestored,
      terminalFailureRecoveryAvailable,
      cancellationStopped,
      cancellationFocusRestored,
      cancellationRecoveryAvailable,
      cancellationMs,
      landingFocusRestored,
      discardedScanId: completedScanId,
      staleScanRejected,
      rescanCompleted,
      rescanResultFocused,
      rescanRows,
      rescanChartSegments,
      scanDetailsPresent,
      backendLabel: initialBackendLabel,
      rescanBackendLabel,
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
            "chartArrowMoved": true,
            "chartHomeMoved": true,
            "listTabStops": 2,
            "listArrowMoved": true,
            "revealArrowPreserved": true,
            "horizontalOverflow": false,
            "pageErrors": [],
            "logicalMetricSelected": true,
            "navigationChangedFolder": true,
            "searchStatus": "10 matches",
            "searchedRows": 10,
            "terminalFailureShown": true,
            "terminalFailureFocusRestored": true,
            "terminalFailureRecoveryAvailable": true,
            "cancellationStopped": true,
            "cancellationFocusRestored": true,
            "cancellationRecoveryAvailable": true,
            "cancellationMs": 50.0,
            "landingFocusRestored": true,
            "discardedScanId": 2,
            "staleScanRejected": true,
            "rescanCompleted": true,
            "rescanResultFocused": true,
            "rescanRows": 4,
            "rescanChartSegments": 8,
            "scanDetailsPresent": true,
            "backendLabel": "macOS native",
            "rescanBackendLabel": "macOS native",
        });
        assert!(validate_report(&complete, 2));

        let mut broken_keyboard = complete.clone();
        broken_keyboard["revealArrowPreserved"] = false.into();
        assert!(!validate_report(&broken_keyboard, 2));

        let mut stale_scan_retained = complete.clone();
        stale_scan_retained["staleScanRejected"] = false.into();
        assert!(!validate_report(&stale_scan_retained, 2));

        assert!(!validate_report(&complete, 1));

        let mut terminal_failure_focus_lost = complete.clone();
        terminal_failure_focus_lost["terminalFailureFocusRestored"] = false.into();
        assert!(!validate_report(&terminal_failure_focus_lost, 2));

        let mut cancellation_focus_lost = complete.clone();
        cancellation_focus_lost["cancellationFocusRestored"] = false.into();
        assert!(!validate_report(&cancellation_focus_lost, 2));

        let mut overflow = complete;
        overflow["horizontalOverflow"] = true.into();
        assert!(!validate_report(&overflow, 2));
    }
}
