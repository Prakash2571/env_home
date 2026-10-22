import type { AccountView, Drive, Phase } from "../api/types";
import { ago, duration, istClock, istDate } from "../format";
import { StageChain } from "./StageChain";

const PHASE: Record<Phase, { label: string; tone: string }> = {
  idle: { label: "no token yet", tone: "dim" },
  logging_in: { label: "logging in", tone: "running" },
  awaiting_callback: { label: "waiting for you", tone: "warn" },
  ready: { label: "ready", tone: "done" },
  failed: { label: "failed", tone: "failed" },
  stopped_for_today: { label: "stopped for today", tone: "failed" },
};

interface AccountCardProps {
  account: AccountView;
  now: number;
  automation: boolean;
  busy: boolean;
  onLogin: (alias: string, drive: Drive) => void;
}

function TokenLine({ account, now }: { account: AccountView; now: number }) {
  const token = account.token;
  if (token === null) {
    return <p className="muted">No token stored for this account.</p>;
  }
  const left = token.expires_at - now;
  return (
    <dl className="facts">
      <div>
        <dt>token</dt>
        <dd className={left > 0 ? "tone-done" : "tone-failed"}>
          {left > 0 ? `valid for ${duration(left)}` : "expired"}
        </dd>
      </div>
      <div>
        <dt>minted</dt>
        <dd>
          {istDate(token.obtained_at)} {istClock(token.obtained_at)} IST · {token.drive}
        </dd>
      </div>
      <div>
        <dt>last served</dt>
        <dd>{account.last_served_at === null ? "never" : ago(account.last_served_at, now)}</dd>
      </div>
    </dl>
  );
}

export function AccountCard({ account, now, automation, busy, onLogin }: AccountCardProps) {
  const phase = PHASE[account.phase];
  const running = account.phase === "logging_in" || account.phase === "awaiting_callback";
  return (
    <article className="card" aria-label={`account ${account.alias}`}>
      <header className="card-head">
        <div>
          <h2>{account.alias}</h2>
          <p className="muted mono">
            client {account.client_id_hint} · API seed {account.api_seed}
          </p>
        </div>
        <span className={`pill tone-${phase.tone}`}>{phase.label}</span>
      </header>

      <StageChain stages={account.stages} now={now} />

      {account.consent_url === null ? null : (
        <p className="consent">
          <a href={account.consent_url} target="_blank" rel="noreferrer noopener">
            Open the Dhan consent page
          </a>{" "}
          <span className="muted">to finish this login yourself</span>
        </p>
      )}
      {account.last_error === null ? null : <p className="error">{account.last_error}</p>}

      <TokenLine account={account} now={now} />

      <div className="actions">
        <button
          type="button"
          className="action primary"
          disabled={busy || running || !automation}
          title={automation ? "The sidecar fills the account, PIN and TOTP" : "CBL_SIDECAR_URL is not set"}
          onClick={() => onLogin(account.alias, "automated")}
        >
          Run automated login
        </button>
        <button
          type="button"
          className="action"
          disabled={busy || running}
          title="Publishes a consent link for you to open; five minutes to finish"
          onClick={() => onLogin(account.alias, "manual")}
        >
          Manual login
        </button>
      </div>
    </article>
  );
}
