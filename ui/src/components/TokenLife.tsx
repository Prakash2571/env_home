import type { TokenView } from "../api/types";
import { duration } from "../format";

/** Dhan tokens last about a day; the bar shows how much of it is left. */
const TOKEN_LIFETIME_SECONDS = 86_400;

export function TokenLife({ token, now }: { token: TokenView | null; now: number }) {
  if (token === null) {
    return <span className="token-life muted">no token yet</span>;
  }
  const left = token.expires_at - now;
  if (left <= 0) {
    return <span className="token-life tone-failed">expired</span>;
  }
  const share = Math.min(1, left / TOKEN_LIFETIME_SECONDS);
  return (
    <span className="token-life">
      <span className="tone-done mono">valid for {duration(left)}</span>
      <span className="life-bar" aria-hidden="true">
        <span className="life-fill" style={{ width: `${Math.round(share * 100)}%` }} />
      </span>
    </span>
  );
}
