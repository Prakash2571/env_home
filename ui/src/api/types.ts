export type StageId = "consent" | "browser" | "callback" | "exchange" | "stored" | "notified";
export type StageStatus = "idle" | "running" | "done" | "failed" | "skipped";
export type Phase = "idle" | "logging_in" | "awaiting_callback" | "ready" | "failed" | "stopped_for_today";
export type EventKind = "login" | "problem" | "api" | "system";
export type Drive = "automated" | "manual";

export interface StageView {
  id: StageId;
  label: string;
  status: StageStatus;
  detail: string;
  started_at: number | null;
  finished_at: number | null;
}

export interface TokenView {
  obtained_at: number;
  expires_at: number;
  drive: string;
}

export interface AccountView {
  alias: string;
  client_id_hint: string;
  phase: Phase;
  stages: StageView[];
  last_error: string | null;
  consent_url: string | null;
  token: TokenView | null;
  last_served_at: number | null;
}

export interface EventView {
  at: number;
  kind: EventKind;
  alias: string | null;
  message: string;
}

export interface RequestRecord {
  at: number;
  client_id: string;
  alias: string | null;
  ip: string | null;
  outcome: string;
  reason: string;
}

export interface AttemptRecord {
  alias: string;
  drive: string;
  started_at: number;
  finished_at: number;
  outcome: string;
  stage: string;
  detail: string;
  consent_spent: boolean;
}

export interface TelegramView {
  configured: boolean;
  delivered: number;
  dropped: number;
  last_error: string | null;
  last_delivered_at: number | null;
}

export interface WindowView {
  label: string;
  open: boolean;
  opens_in: number;
  closes_in: number;
}

export interface Snapshot {
  configured: boolean;
  problems: string[];
  version: string;
  started_at: number;
  generated_at: number;
  store: string | null;
  store_reachable: boolean;
  automation: boolean;
  accounts_loaded: boolean;
  api_requires_access_jwt: boolean;
  window: WindowView | null;
  accounts: AccountView[];
  telegram: TelegramView;
  events: EventView[];
  requests: RequestRecord[];
  attempts: AttemptRecord[];
}

/** What the dashboard sends to add a Dhan account. Never stored in the page. */
export interface AccountForm {
  alias: string;
  client_id: string;
  api_key: string;
  api_secret: string;
  account_id: string;
  pin: string;
  dhan_totp_secret: string;
}

export type AccountField = keyof AccountForm;

export interface FieldProblem {
  field: AccountField;
  message: string;
}

/** The token API seed, returned once when an account is added or re-keyed. */
export interface AddedAccount {
  alias: string;
  token_api_seed: string;
  otpauth_uri: string;
}
