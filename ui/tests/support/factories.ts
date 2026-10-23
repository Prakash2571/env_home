import type { AccountView, Snapshot, StageView } from "../../src/api/types";

export const NOW = 1_792_656_000;

export function stages(statuses: StageView["status"][] = []): StageView[] {
  const ids: StageView["id"][] = ["consent", "browser", "callback", "exchange", "stored", "notified"];
  return ids.map((id, index) => ({
    id,
    label: id,
    status: statuses[index] ?? "idle",
    detail: "",
    started_at: null,
    finished_at: null,
  }));
}

export function account(overrides: Partial<AccountView> = {}): AccountView {
  return {
    alias: "main",
    client_id_hint: "••••0001",
    phase: "idle",
    stages: stages(),
    last_error: null,
    consent_url: null,
    token: null,
    last_served_at: null,
    ...overrides,
  };
}

export function snapshot(overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    configured: true,
    problems: [],
    version: "0.1.0",
    started_at: NOW - 3600,
    generated_at: NOW,
    store: "postgres",
    store_reachable: true,
    automation: true,
    accounts_loaded: true,
    api_requires_access_jwt: false,
    window: { label: "08:00-08:30 IST", open: false, opens_in: 3600, closes_in: 0 },
    accounts: [account()],
    telegram: { configured: true, delivered: 3, dropped: 0, last_error: null, last_delivered_at: NOW },
    events: [],
    requests: [],
    attempts: [],
    ...overrides,
  };
}
