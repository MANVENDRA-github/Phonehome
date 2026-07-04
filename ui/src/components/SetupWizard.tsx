import { useState } from "react";
import { api } from "../api";
import { buildSourceBody, validateSource, type SourceKind } from "../setup";

type TestState =
  | { status: "idle" }
  | { status: "testing" }
  | { status: "ok" }
  | { status: "error"; message: string };

/** First-run setup wizard (M5): paste a Pi-hole / AdGuard URL + token, test the
 * connection live, then start ingesting. Shown full-bleed by App when
 * `config.needs_setup` is true. On success `onDone` refetches config so the app
 * transitions to the globe. */
export function SetupWizard({ onDone }: { onDone: () => void }) {
  const [kind, setKind] = useState<SourceKind>("pihole");
  const [baseUrl, setBaseUrl] = useState("");
  const [username, setUsername] = useState("");
  const [secret, setSecret] = useState("");
  const [showHome, setShowHome] = useState(false);
  const [homeLat, setHomeLat] = useState("");
  const [homeLon, setHomeLon] = useState("");
  const [test, setTest] = useState<TestState>({ status: "idle" });
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const form = { kind, baseUrl, username, secret };
  const validation = validateSource(form);
  const canTest = validation.ok && test.status !== "testing";
  const canSubmit = test.status === "ok" && !saving;

  // Any input change invalidates a prior green test — you must re-test.
  const dirty = () => {
    if (test.status !== "idle") setTest({ status: "idle" });
    if (saveError) setSaveError(null);
  };

  async function runTest() {
    setTest({ status: "testing" });
    try {
      const res = await api.testSource(buildSourceBody(form));
      const body = await res.json().catch(() => ({}));
      if (res.ok && body.ok) setTest({ status: "ok" });
      else
        setTest({
          status: "error",
          message: body.error ?? `the daemon returned HTTP ${res.status}`,
        });
    } catch {
      setTest({ status: "error", message: "could not reach the daemon" });
    }
  }

  async function submit() {
    setSaving(true);
    setSaveError(null);
    try {
      const res = await api.saveSource(
        buildSourceBody(form, showHome ? { lat: homeLat, lon: homeLon } : undefined),
      );
      if (res.ok) {
        onDone();
        return;
      }
      const body = await res.json().catch(() => ({}));
      setSaveError(body.error ?? `the daemon returned HTTP ${res.status}`);
    } catch {
      setSaveError("could not reach the daemon");
    } finally {
      setSaving(false);
    }
  }

  const kindBtn = (k: SourceKind, label: string, testid: string) => (
    <button
      type="button"
      data-testid={testid}
      onClick={() => {
        setKind(k);
        dirty();
      }}
      className={`flex-1 rounded-lg border px-4 py-2 text-sm font-medium transition ${
        kind === k
          ? "border-emerald-500/60 bg-emerald-500/10 text-emerald-300"
          : "border-slate-700 bg-slate-900/40 text-slate-400 hover:text-slate-200"
      }`}
    >
      {label}
    </button>
  );

  const inputCls =
    "w-full rounded-lg border border-slate-700 bg-slate-950/60 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-600 focus:border-emerald-500/60 focus:outline-none";

  return (
    <main className="flex min-h-screen items-center justify-center bg-[#060a12] px-6 py-10">
      <div
        data-testid="setup-wizard"
        className="w-full max-w-md rounded-xl border border-slate-800 bg-slate-900/40 p-6 shadow-xl"
      >
        <div className="mb-1 flex items-center gap-2">
          <h1 className="text-2xl font-bold tracking-tight text-slate-100">phonehome</h1>
          <span className="inline-block h-2.5 w-2.5 rounded-full bg-emerald-400" />
        </div>
        <p className="mb-6 text-sm text-slate-400">
          Point it at the DNS filter you already run. Nothing leaves your network.
        </p>

        <div className="mb-4 flex gap-2">
          {kindBtn("pihole", "Pi-hole", "setup-kind-pihole")}
          {kindBtn("adguard", "AdGuard Home", "setup-kind-adguard")}
        </div>

        <div className="flex flex-col gap-3">
          <label className="flex flex-col gap-1">
            <span className="text-xs font-medium text-slate-400">Address</span>
            <input
              data-testid="setup-url"
              className={inputCls}
              placeholder={kind === "pihole" ? "http://pi.hole" : "http://adguard.local"}
              value={baseUrl}
              onChange={(e) => {
                setBaseUrl(e.target.value);
                dirty();
              }}
            />
          </label>

          {kind === "adguard" && (
            <label className="flex flex-col gap-1">
              <span className="text-xs font-medium text-slate-400">Username</span>
              <input
                data-testid="setup-username"
                className={inputCls}
                placeholder="admin"
                value={username}
                onChange={(e) => {
                  setUsername(e.target.value);
                  dirty();
                }}
              />
            </label>
          )}

          <label className="flex flex-col gap-1">
            <span className="text-xs font-medium text-slate-400">
              {kind === "pihole" ? "App password" : "Password"}
            </span>
            <input
              data-testid="setup-secret"
              type="password"
              className={inputCls}
              placeholder="••••••••"
              value={secret}
              onChange={(e) => {
                setSecret(e.target.value);
                dirty();
              }}
            />
          </label>

          {!showHome ? (
            <button
              type="button"
              className="self-start text-xs text-slate-500 hover:text-slate-300"
              onClick={() => setShowHome(true)}
            >
              + place your home on the globe (optional)
            </button>
          ) : (
            <div className="flex gap-2">
              <input
                data-testid="setup-home-lat"
                className={inputCls}
                placeholder="lat e.g. 12.97"
                value={homeLat}
                onChange={(e) => setHomeLat(e.target.value)}
              />
              <input
                data-testid="setup-home-lon"
                className={inputCls}
                placeholder="lon e.g. 77.59"
                value={homeLon}
                onChange={(e) => setHomeLon(e.target.value)}
              />
            </div>
          )}
        </div>

        {/* Live test-connection feedback (emerald ok / rose error). */}
        <div data-testid="setup-test-result" className="mt-4 min-h-[1.25rem] text-xs">
          {test.status === "testing" && <span className="text-slate-400">testing…</span>}
          {test.status === "ok" && (
            <span className="text-emerald-400">✓ connected — ready to start</span>
          )}
          {test.status === "error" && <span className="text-rose-400">✕ {test.message}</span>}
          {test.status === "idle" && !validation.ok && baseUrl.length > 0 && (
            <span className="text-slate-600">{validation.error}</span>
          )}
        </div>

        <div className="mt-3 flex gap-2">
          <button
            type="button"
            data-testid="setup-test"
            disabled={!canTest}
            onClick={runTest}
            className="flex-1 rounded-lg border border-slate-700 px-4 py-2 text-sm font-medium text-slate-200 transition enabled:hover:border-slate-500 disabled:opacity-40"
          >
            Test connection
          </button>
          <button
            type="button"
            data-testid="setup-submit"
            disabled={!canSubmit}
            onClick={submit}
            className="flex-1 rounded-lg bg-emerald-500 px-4 py-2 text-sm font-semibold text-slate-950 transition enabled:hover:bg-emerald-400 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {saving ? "starting…" : "Start"}
          </button>
        </div>

        {saveError && <p className="mt-3 text-xs text-rose-400">{saveError}</p>}
      </div>
    </main>
  );
}
