import { useEffect, useState } from "react";
import { requestLogin, testTelegram } from "./api/client";
import type { Drive, Snapshot } from "./api/types";
import { useSnapshot } from "./api/useSnapshot";
import { AccountCard } from "./components/AccountCard";
import { AccountsPage } from "./components/AccountsPage";
import { AttemptLog, EventLog, RequestLog } from "./components/Logs";
import { StatusStrip } from "./components/StatusStrip";

function useNow(): number {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, []);
  return now;
}

type View = "logins" | "accounts";

const VIEWS: { view: View; label: string }[] = [
  { view: "logins", label: "Logins" },
  { view: "accounts", label: "Accounts" },
];

function viewFromHash(): View {
  return window.location.hash === "#accounts" ? "accounts" : "logins";
}

function useView(): [View, (view: View) => void] {
  const [view, setView] = useState<View>(viewFromHash);
  useEffect(() => {
    const follow = () => setView(viewFromHash());
    window.addEventListener("hashchange", follow);
    return () => window.removeEventListener("hashchange", follow);
  }, []);
  const show = (next: View) => {
    setView(next);
    window.history.replaceState(null, "", next === "accounts" ? "#accounts" : window.location.pathname);
  };
  return [view, show];
}

function Unconfigured({ snapshot }: { snapshot: Snapshot }) {
  return (
    <section className="panel problems" aria-label="configuration problems">
      <h2>The service is running but not configured</h2>
      <p className="muted">Fix these in the VPS .env, then reload the stack from release_manager/status.sh.</p>
      <ul>
        {snapshot.problems.map((problem) => (
          <li key={problem} className="mono">
            {problem}
          </li>
        ))}
      </ul>
    </section>
  );
}

interface LoginsProps {
  snapshot: Snapshot;
  now: number;
  busy: boolean;
  onLogin: (alias: string, drive: Drive) => void;
  onAdd: () => void;
}

function Logins({ snapshot, now, busy, onLogin, onAdd }: LoginsProps) {
  return (
    <>
      {snapshot.accounts.length === 0 ? (
        <section className="panel empty" aria-label="no accounts">
          <p>{snapshot.accounts_loaded ? "No Dhan account yet." : "Loading the stored accounts…"}</p>
          {snapshot.accounts_loaded ? (
            <button type="button" className="action primary" onClick={onAdd}>
              Add an account
            </button>
          ) : null}
        </section>
      ) : (
        <section className="cards" aria-label="accounts">
          {snapshot.accounts.map((account) => (
            <AccountCard
              key={account.alias}
              account={account}
              now={now}
              automation={snapshot.automation}
              busy={busy}
              onLogin={onLogin}
            />
          ))}
        </section>
      )}
      <div className="grid">
        <EventLog events={snapshot.events} />
        <RequestLog requests={snapshot.requests} />
      </div>
      <AttemptLog attempts={snapshot.attempts} />
    </>
  );
}

export function App() {
  const { snapshot, link, error } = useSnapshot();
  const now = useNow();
  const [view, show] = useView();
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const run = async (work: () => Promise<void>, done: string) => {
    setBusy(true);
    setNotice(null);
    try {
      await work();
      setNotice(done);
    } catch (cause) {
      setNotice(cause instanceof Error ? cause.message : "the request failed");
    } finally {
      setBusy(false);
    }
  };

  const onLogin = (alias: string | null, drive: Drive) =>
    run(() => requestLogin(alias, drive), `${drive} login queued for ${alias ?? "every account"}`);

  return (
    <div className="shell">
      <header className="top">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            ◆
          </span>
          <div>
            <h1>Broker Login</h1>
            <p className="muted">centralised logins and the token vault</p>
          </div>
        </div>
        {snapshot?.configured ? (
          <div className="top-actions">
            <button
              type="button"
              className="action"
              disabled={busy || !snapshot.automation}
              title={snapshot.automation ? "Runs the sidecar for each account in turn" : "CBL_SIDECAR_URL is not set"}
              onClick={() => onLogin(null, "automated")}
            >
              Log in every account
            </button>
            <button
              type="button"
              className="action"
              disabled={busy || !snapshot.telegram.configured}
              onClick={() => run(testTelegram, "test notice queued")}
            >
              Test Telegram
            </button>
          </div>
        ) : null}
      </header>

      {notice === null ? null : (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      {snapshot === null ? (
        <p className={link === "denied" ? "error" : "muted"}>
          {link === "denied" ? "Sign in through Cloudflare Access to see this page." : error ?? "Loading…"}
        </p>
      ) : (
        <main>
          <StatusStrip snapshot={snapshot} link={link} />
          {!snapshot.configured ? (
            <Unconfigured snapshot={snapshot} />
          ) : (
            <>
              <nav className="tabs" aria-label="pages">
                {VIEWS.map((tab) => (
                  <button
                    key={tab.view}
                    type="button"
                    className="tab"
                    aria-current={view === tab.view ? "page" : undefined}
                    onClick={() => show(tab.view)}
                  >
                    {tab.label}
                  </button>
                ))}
              </nav>
              {view === "accounts" ? (
                <AccountsPage accounts={snapshot.accounts} loaded={snapshot.accounts_loaded} now={now} />
              ) : (
                <Logins snapshot={snapshot} now={now} busy={busy} onLogin={onLogin} onAdd={() => show("accounts")} />
              )}
            </>
          )}
          <footer className="muted mono foot">version {snapshot.version}</footer>
        </main>
      )}
    </div>
  );
}
