import type { AccountView, WindowView } from "../api/types";
import { duration } from "../format";
import { hasLiveToken, isRunning, needsAttention } from "./phase";

interface OverviewProps {
  accounts: AccountView[];
  window: WindowView | null;
  now: number;
}

function windowText(window: WindowView | null): string {
  if (window === null) {
    return "no login window";
  }
  return window.open
    ? `login window open, ${duration(window.closes_in)} left`
    : `next login window in ${duration(window.opens_in)}`;
}

/** The one question the page answers first: is every account logged in? */
export function Overview({ accounts, window, now }: OverviewProps) {
  const ready = accounts.filter((account) => hasLiveToken(account, now)).length;
  const running = accounts.filter(isRunning).length;
  const attention = accounts.filter((account) => needsAttention(account) && !isRunning(account)).length;
  const allReady = accounts.length > 0 && ready === accounts.length && attention === 0 && running === 0;
  return (
    <section className={`overview ${allReady ? "overview--ok" : ""}`} aria-label="overview">
      <p className="overview-lead">
        <strong className={allReady ? "tone-done" : undefined}>
          {ready} of {accounts.length}
        </strong>{" "}
        {accounts.length === 1 ? "account has" : "accounts have"} a valid token
      </p>
      <ul className="overview-facts">
        {running > 0 ? <li className="tone-running">{running} logging in</li> : null}
        {attention > 0 ? <li className="tone-failed">{attention} need attention</li> : null}
        <li className="muted">{windowText(window)}</li>
      </ul>
    </section>
  );
}
