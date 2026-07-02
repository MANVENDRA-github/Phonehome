# PROOF.md — evidence per milestone

Rule (D-008): every claim, number, and demo asset here comes from a **real run**. If something wasn't verified, it says so explicitly. Command output is pasted, not paraphrased.

---

## §M0 — Scaffold (2026-07-02)

Environment: Windows 11, rustc 1.94.1 (MSVC), Node v20.20.1 / npm 10.8.2.

### UI build (typecheck + bundle) — PASS

```
> phonehome-ui@0.1.0 build
> tsc --noEmit && vite build

vite v6.4.3 building for production...
✓ 29 modules transformed.
dist/index.html                   0.41 kB │ gzip:  0.28 kB
dist/assets/index-BGWB0P7j.css    7.05 kB │ gzip:  2.32 kB
dist/assets/index-CB_NgYZb.js   195.61 kB │ gzip: 61.37 kB
✓ built in 1.23s
```

`npm install` reported **0 vulnerabilities**.

### Rust: fmt + clippy + tests — PASS

`cargo fmt --check` → clean. `cargo clippy --all-targets -- -D warnings` → clean ("Finished `dev` profile … in 1m 05s", zero warnings under `-D warnings`).

```
running 2 tests   (phonehome-core)
test tests::query_event_without_mac_round_trips ... ok
test tests::query_event_serde_round_trip ... ok
test result: ok. 2 passed; 0 failed

running 3 tests   (phonehome-daemon)
test tests::unknown_api_path_is_404_not_spa_fallback ... ok
test tests::health_returns_alive_with_version ... ok
test tests::root_serves_embedded_ui ... ok
test result: ok. 3 passed; 0 failed
```

### Local daemon run — PASS

`cargo run -p phonehome-daemon`, then from another shell:

```
$ curl -s http://localhost:8480/api/health
{"status":"alive","version":"0.1.0"}

$ curl -s http://localhost:8480/ | head -c 400
<!doctype html>
<html lang="en" class="dark">
  <head>
    <meta charset="UTF-8" />
    ...
    <title>Phonehome</title>
    <script type="module" crossorigin src="/assets/index-CB_NgYZb.js"></script>
    <link rel="stylesheet" crossorigin href="/assets/index-BGWB0P7j.css">
```

Bonus (observed, not staged): starting a second instance while the first held the port produced the intended bind panic — `failed to bind 0.0.0.0:8480: Only one usage of each socket address … (os error 10048)`.

### docker compose — PASS in CI; local validation blocked (disclosed)

**Local limitation:** this dev machine cannot currently run Docker Desktop — `wsl --status` reports *"WSL2 is not supported with your current machine configuration. Please enable the 'Virtual Machine Platform' optional component and ensure virtualization is enabled in the BIOS."* Per the honesty rule this is disclosed rather than worked around silently.

**Compensating real proof:** the CI `docker-smoke` job runs `docker compose up -d --build` on a clean Ubuntu runner, polls `GET /api/health` until it returns the alive JSON, and greps the served page for "phonehome". See the CI run linked below.

### CI — PASS

- Run: https://github.com/MANVENDRA-github/Phonehome/actions/runs/28608143084 — `build-test` pass (1m3s: UI build, fmt, clippy, tests) and `docker-smoke` pass (1m0s: compose build + live health/page probes) on PR #2.

**M0 acceptance met:** `cargo test` + `npm run build` green locally and in CI; `docker compose up` serving the alive page proven in CI (local Docker pending machine virtualization — tracked for M5's clean-install test, which needs it anyway).
