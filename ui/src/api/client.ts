import type { AccountForm, AddedAccount, Drive, FieldProblem, Snapshot } from "./types";

export class ApiError extends Error {
  readonly status: number;
  readonly problems: FieldProblem[];

  constructor(status: number, message: string, problems: FieldProblem[] = []) {
    super(message);
    this.status = status;
    this.problems = problems;
  }
}

async function readError(response: Response): Promise<ApiError> {
  const fallback = new ApiError(response.status, `HTTP ${response.status}`);
  try {
    const body = (await response.json()) as { error?: unknown; problems?: unknown };
    if (typeof body.error !== "string") {
      return fallback;
    }
    const problems = Array.isArray(body.problems) ? (body.problems as FieldProblem[]) : [];
    return new ApiError(response.status, body.error, problems);
  } catch {
    return fallback;
  }
}

async function send(method: string, path: string, body: unknown): Promise<Response> {
  const response = await fetch(path, {
    method,
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    credentials: "same-origin",
    cache: "no-store",
  });
  if (!response.ok) {
    throw await readError(response);
  }
  return response;
}

async function post(path: string, body: unknown): Promise<void> {
  await send("POST", path, body);
}

export async function fetchSnapshot(): Promise<Snapshot> {
  const response = await fetch("/api/state", { credentials: "same-origin", cache: "no-store" });
  if (!response.ok) {
    throw await readError(response);
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

const accountPath = (alias: string) => `/api/accounts/${encodeURIComponent(alias)}`;

export async function addAccount(form: AccountForm): Promise<AddedAccount> {
  return (await (await send("POST", "/api/accounts", form)).json()) as AddedAccount;
}

/** `confirm` is the alias as the operator typed it; the server checks it too. */
export async function removeAccount(alias: string, confirm: string): Promise<void> {
  await send("DELETE", accountPath(alias), { confirm });
}

export async function rotateTokenSeed(alias: string, confirm: string): Promise<AddedAccount> {
  return (await (await send("POST", `${accountPath(alias)}/token-seed`, { confirm })).json()) as AddedAccount;
}
