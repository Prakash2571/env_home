import type { Drive, Snapshot } from "./types";

export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function readError(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as { error?: unknown };
    if (typeof body.error === "string") {
      return body.error;
    }
  } catch {
    return `HTTP ${response.status}`;
  }
  return `HTTP ${response.status}`;
}

async function post(path: string, body: unknown): Promise<void> {
  const response = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    credentials: "same-origin",
  });
  if (!response.ok) {
    throw new ApiError(response.status, await readError(response));
  }
}

export async function fetchSnapshot(): Promise<Snapshot> {
  const response = await fetch("/api/state", { credentials: "same-origin", cache: "no-store" });
  if (!response.ok) {
    throw new ApiError(response.status, await readError(response));
  }
  return (await response.json()) as Snapshot;
}

export function requestLogin(alias: string | null, drive: Drive): Promise<void> {
  const path = alias === null ? "/api/accounts/login" : `/api/accounts/${encodeURIComponent(alias)}/login`;
  return post(path, { drive });
}

export function testTelegram(): Promise<void> {
  return post("/api/telegram/test", {});
}
