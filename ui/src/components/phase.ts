import type { AccountView, Phase } from "../api/types";

export const PHASE: Record<Phase, { label: string; tone: string }> = {
  idle: { label: "no token yet", tone: "dim" },
  logging_in: { label: "logging in", tone: "running" },
  awaiting_callback: { label: "waiting for you", tone: "warn" },
  ready: { label: "ready", tone: "done" },
  failed: { label: "failed", tone: "failed" },
  stopped_for_today: { label: "stopped for today", tone: "failed" },
};

/** A login in flight, or one a person has to look at: its details open by themselves. */
export function needsAttention(account: AccountView): boolean {
  return (
    account.phase === "logging_in" ||
    account.phase === "awaiting_callback" ||
    account.phase === "failed" ||
    account.phase === "stopped_for_today" ||
    account.consent_url !== null
  );
}

export function isRunning(account: AccountView): boolean {
  return account.phase === "logging_in" || account.phase === "awaiting_callback";
}

/** A token that is still valid: the account is served to engines. */
export function hasLiveToken(account: AccountView, now: number): boolean {
  return account.token !== null && account.token.expires_at > now;
}
