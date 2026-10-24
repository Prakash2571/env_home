import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AccountRow } from "../src/components/AccountRow";
import { Overview } from "../src/components/Overview";
import { AttemptLog, EventLog, RequestLog } from "../src/components/Logs";
import { StageChain } from "../src/components/StageChain";
import { StatusStrip } from "../src/components/StatusStrip";
import { NOW, account, snapshot, stages } from "./support/factories";

describe("StageChain", () => {
  it("draws every stage with its state and how long it took", () => {
    const chain = stages(["done", "running", "idle", "idle", "idle", "skipped"]);
    chain[0] = { ...chain[0]!, started_at: NOW - 5, finished_at: NOW - 3, detail: "consent generated" };
    chain[1] = { ...chain[1]!, started_at: NOW - 12, detail: "typing the PIN" };
    render(<StageChain stages={chain} now={NOW} />);
    const items = within(screen.getByRole("list", { name: "login stages" })).getAllByRole("listitem");
    expect(items).toHaveLength(6);
    expect(items[0]).toHaveClass("stage--done");
    expect(items[0]).toHaveTextContent("done · 2s");
    expect(items[0]).not.toHaveTextContent("consent generated");
    expect(items[0], "a finished step keeps its detail in the tooltip").toHaveAttribute("title", "consent generated");
    expect(items[1]).toHaveTextContent("running · 12s");
    expect(items[1], "a running step shows what it is doing").toHaveTextContent("typing the PIN");
    expect(items[2]).toHaveTextContent("waiting");
    expect(items[5]).toHaveTextContent("skipped");
    expect(items[5]).not.toHaveTextContent("·");
  });
});

describe("AccountRow", () => {
  it("shows the token's remaining life, the last login and never the whole client ID", () => {
    const view = account({
      phase: "ready",
      stages: stages(["done", "done", "done", "done", "done", "done"]),
      token: { obtained_at: NOW - 600, expires_at: NOW + 7200, drive: "scheduled" },
      last_served_at: NOW - 30,
    });
    render(<AccountRow account={view} now={NOW} automation busy={false} onLogin={() => {}} />);
    const row = screen.getByRole("article", { name: "account main" });
    expect(within(row).getByText("ready")).toBeInTheDocument();
    expect(within(row).getByText("valid for 2h 0m")).toBeInTheDocument();
    expect(within(row).getByText("served 30s ago")).toBeInTheDocument();
    expect(within(row).getByText(/client ••••0001/)).toBeInTheDocument();
    expect(within(row).getByText(/today .* IST · scheduled/)).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "login stages" }), "every account shows its flow chart").toBeInTheDocument();
    expect(screen.queryByText(/not kept across a restart/), "its steps are known here").toBeNull();
  });

  it("says why a ready account's chart is empty after a restart", () => {
    const view = account({ phase: "ready", token: { obtained_at: NOW - 600, expires_at: NOW + 7200, drive: "scheduled" } });
    render(<AccountRow account={view} now={NOW} automation busy={false} onLogin={() => {}} />);
    expect(screen.getByText(/not kept across a restart; its token is/)).toBeInTheDocument();
  });

  it("shows a failure with its consent link and Dhan's error", () => {
    const view = account({
      phase: "stopped_for_today",
      last_error: "browser: Dhan said: Too many attempts",
      consent_url: "https://auth.dhan.co/login/consentApp-login?consentAppId=c-1",
      token: { obtained_at: NOW - 90_000, expires_at: NOW - 10, drive: "manual" },
    });
    render(<AccountRow account={view} now={NOW} automation busy={false} onLogin={() => {}} />);
    expect(screen.getByText("stopped for today")).toBeInTheDocument();
    expect(screen.getByText("expired")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "login stages" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open the Dhan consent page" })).toHaveAttribute("rel", "noreferrer noopener");
    expect(screen.getByText(/Too many attempts/)).toBeInTheDocument();
  });

  it("asks for the right drive and disables what cannot run", async () => {
    const onLogin = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(<AccountRow account={account()} now={NOW} automation busy={false} onLogin={onLogin} />);
    await user.click(screen.getByRole("button", { name: "Run automated login" }));
    await user.click(screen.getByRole("button", { name: "Manual login" }));
    expect(onLogin.mock.calls).toEqual([
      ["main", "automated"],
      ["main", "manual"],
    ]);
    expect(screen.getByText("no token yet", { selector: ".token-life" })).toBeInTheDocument();


    rerender(<AccountRow account={account()} now={NOW} automation={false} busy={false} onLogin={onLogin} />);
    expect(screen.getByRole("button", { name: "Run automated login" })).toBeDisabled();
    rerender(<AccountRow account={account({ phase: "awaiting_callback" })} now={NOW} automation busy={false} onLogin={onLogin} />);
    expect(screen.getByRole("button", { name: "Manual login" })).toBeDisabled();
  });
});

describe("Overview", () => {
  it("counts the accounts with a valid token and the ones that need someone", () => {
    const valid = { obtained_at: NOW - 60, expires_at: NOW + 3600, drive: "scheduled" };
    const { rerender } = render(
      <Overview
        accounts={[
          account({ alias: "a", phase: "ready", token: valid }),
          account({ alias: "b", phase: "failed" }),
          account({ alias: "c", phase: "logging_in" }),
        ]}
        window={{ label: "07:45-08:30 IST", open: false, opens_in: 3600, closes_in: 0 }}
        now={NOW}
      />,
    );
    const overview = screen.getByRole("region", { name: "overview" });
    expect(overview).toHaveTextContent("1 of 3 accounts have a valid token");
    expect(overview).toHaveTextContent("1 logging in");
    expect(overview).toHaveTextContent("1 need attention");
    expect(overview).not.toHaveClass("overview--ok");
    expect(overview).toHaveTextContent("next login window in 1h 0m");

    rerender(
      <Overview
        accounts={[account({ phase: "ready", token: valid })]}
        window={{ label: "x", open: true, opens_in: 0, closes_in: 90 }}
        now={NOW}
      />,
    );
    expect(overview).toHaveTextContent("1 of 1 account has a valid token");
    expect(overview).toHaveTextContent("login window open, 1m 30s left");
    expect(overview).toHaveClass("overview--ok");
  });
});

describe("StatusStrip", () => {
  it("summarises the window, store, sidecar, Telegram and API mode", () => {
    const view = snapshot({
      window: { label: "08:00-08:30 IST", open: true, opens_in: 0, closes_in: 125 },
      store_reachable: false,
      automation: false,
      api_requires_access_jwt: true,
      telegram: { configured: true, delivered: 0, dropped: 0, last_error: "Telegram returned HTTP 400", last_delivered_at: null },
    });
    render(<StatusStrip snapshot={view} link="reconnecting" />);
    const strip = screen.getByRole("list", { name: "service status" });
    expect(strip).toHaveTextContent("reconnecting");
    expect(strip).toHaveTextContent("open, 2m 5s left");
    expect(strip).toHaveTextContent("postgres (unreachable)");
    expect(strip).toHaveTextContent("manual only");
    expect(strip).toHaveTextContent("Telegram returned HTTP 400");
    expect(strip).toHaveTextContent("service token + TOTP");
  });

  it("handles a service without a window or Telegram", () => {
    render(<StatusStrip snapshot={snapshot({ window: null, store: null, telegram: { configured: false, delivered: 0, dropped: 0, last_error: null, last_delivered_at: null } })} link="live" />);
    const strip = screen.getByRole("list", { name: "service status" });
    expect(strip).toHaveTextContent("not configured");
    expect(strip).toHaveTextContent("live");
  });
});

describe("logs", () => {
  it("lists events, requests and attempts, and says when there are none", () => {
    const { rerender } = render(<EventLog events={[]} now={NOW} />);
    expect(screen.getByText("Nothing has happened yet.")).toBeInTheDocument();
    rerender(<EventLog events={[{ at: NOW, kind: "problem", alias: "main", message: "login failed" }]} now={NOW} />);
    expect(screen.getByText("login failed")).toBeInTheDocument();

    render(<RequestLog requests={[{ at: NOW, client_id: "1", alias: "main", ip: "203.0.113.7", outcome: "served", reason: "valid for 20h" }]} />);
    expect(screen.getByText("203.0.113.7")).toBeInTheDocument();
    expect(screen.getByText("served")).toHaveClass("tone-done");

    render(<AttemptLog now={NOW} attempts={[{ alias: "main", drive: "scheduled", started_at: NOW, finished_at: NOW + 42, outcome: "failed", stage: "browser", detail: "Invalid TOTP", consent_spent: true }]} />);
    expect(screen.getByText("failed at browser")).toHaveClass("tone-failed");
    expect(screen.getByText("42s")).toBeInTheDocument();
  });

  it("groups login attempts and activity by IST day, newest first", () => {
    const attempt = (started_at: number, alias: string) => ({
      alias,
      drive: "scheduled",
      started_at,
      finished_at: started_at + 30,
      outcome: "ok",
      stage: "notified",
      detail: "token stored",
      consent_spent: true,
    });
    render(
      <AttemptLog
        now={NOW}
        attempts={[attempt(NOW - 60, "today_one"), attempt(NOW - 86_400, "yesterday_one"), attempt(NOW - 3 * 86_400, "older_one")]}
      />,
    );
    const table = screen.getByRole("table", { name: "login attempts by day" });
    const headings = within(table).getAllByRole("rowheader").map((cell) => cell.textContent);
    expect(headings[0]).toMatch(/^Today · /);
    expect(headings[1]).toMatch(/^Yesterday · /);
    expect(headings[2]).toMatch(/^[A-Z][a-z]{2} \d{1,2} [A-Z][a-z]{2}$/);
    expect(within(table).getAllByRole("row").map((row) => row.textContent)[2]).toContain("today_one");
    expect(within(table).queryByText(/^\d\d-\d\d /), "inside a day only the time is shown").toBeNull();

    render(
      <EventLog
        now={NOW}
        events={[
          { at: NOW - 30, kind: "api", alias: "main", message: "token served" },
          { at: NOW - 86_400, kind: "login", alias: "main", message: "logged in" },
        ]}
      />,
    );
    const activity = screen.getByRole("region", { name: "activity" });
    const days = within(activity).getAllByRole("heading", { level: 4 }).map((heading) => heading.textContent);
    expect(days).toEqual([expect.stringMatching(/^Today · /), expect.stringMatching(/^Yesterday · /)]);
  });

  it("says when no engine or login has run", () => {
    render(<RequestLog requests={[]} />);
    render(<AttemptLog attempts={[]} now={NOW} />);
    expect(screen.getByText("No engine has asked for a token yet.")).toBeInTheDocument();
    expect(screen.getByText("No login has run yet.")).toBeInTheDocument();
  });
});
