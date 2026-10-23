import type { AccountView, Drive } from "../api/types";
import { ago, istClock, istDate } from "../format";
import { PHASE, isRunning } from "./phase";
import { StageChain } from "./StageChain";
import { TokenLife } from "./TokenLife";

interface AccountRowProps {
  account: AccountView;
  now: number;
  automation: boolean;
  busy: boolean;
  onLogin: (alias: string, drive: Drive) => void;
}

function lastLogin(account: AccountView, now: number): string {
  const token = account.token;
  if (token === null) {
    return "never logged in";
  }
  const day = istDate(token.obtained_at) === istDate(now) ? "today" : istDate(token.obtained_at);
  return `${day} ${istClock(token.obtained_at).slice(0, 5)} IST · ${token.drive}`;
}

export function AccountRow({ account, now, automation, busy, onLogin }: AccountRowProps) {
  const phase = PHASE[account.phase];
  const running = isRunning(account);
  // A restart keeps the token but not the steps that minted it.
  const stepsForgotten =
    account.token !== null && !running && account.stages.every((stage) => stage.status === "idle");

  return (
    <article className={`row row--${phase.tone}`} aria-label={`account ${account.alias}`}>
      <div className="row-info">
        <div className="row-id">
          <div>
            <h2>{account.alias}</h2>
            <span className="muted mono">client {account.client_id_hint}</span>
          </div>
          <span className={`pill tone-${phase.tone}`}>{phase.label}</span>
        </div>
        <TokenLife token={account.token} now={now} />
        <div className="row-last">
          <span>{lastLogin(account, now)}</span>
          <span className="muted">
            served {account.last_served_at === null ? "never" : ago(account.last_served_at, now)}
          </span>
        </div>
        <div className="row-actions">
          <button
            type="button"
            className="action primary"
            disabled={busy || running || !automation}
            title={
              automation
                ? "The sidecar fills the account, PIN and TOTP; up to 5 tries, then Telegram asks for a manual login"
                : "CBL_SIDECAR_URL is not set"
            }
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
      </div>

      <div className="row-flow">
        <StageChain stages={account.stages} now={now} />
        {stepsForgotten ? (
          <p className="chain-note muted">
            The steps of the last login are not kept across a restart; its token is. They fill in on the next login.
          </p>
        ) : null}
        {account.consent_url === null ? null : (
            <p className="consent">
              <a href={account.consent_url} target="_blank" rel="noreferrer noopener">
                Open the Dhan consent page
              </a>{" "}
              <span className="muted">to finish this login yourself</span>
            </p>
          )}
        {account.last_error === null ? null : <p className="error">{account.last_error}</p>}
      </div>
    </article>
  );
}
