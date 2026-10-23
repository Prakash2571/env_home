import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AccountCard } from "../src/components/AccountCard";
import { AttemptLog, EventLog, RequestLog } from "../src/components/Logs";
import { StageChain } from "../src/components/StageChain";
import { StatusStrip } from "../src/components/StatusStrip";
import { NOW, account, snapshot, stages } from "./support/factories";

describe("StageChain", () => {
  it("draws every stage with its state and how long it took", () => {
    const chain = stages(["done", "running", "idle", "idle", "idle", "skipped"]);
    chain[0] = { ...chain[0]!, started_at: NOW - 5, finished_at: NOW - 3, detail: "consent generated" };
    chain[1] = { ...chain[1]!, started_at: NOW - 12 };
    render(<StageChain stages={chain} now={NOW} />);
    const items = within(screen.getByRole("list", { name: "login stages" })).getAllByRole("listitem");
    expect(items).toHaveLength(6);
    expect(items[0]).toHaveClass("stage--done");
    expect(items[0]).toHaveTextContent("done · 2s");
    expect(items[0]).toHaveTextContent("consent generated");
    expect(items[1]).toHaveTextContent("running · 12s");
    expect(items[2]).toHaveTextContent("waiting");
    expect(items[5]).toHaveTextContent("skipped");
    expect(items[5]).not.toHaveTextContent("·");
  });
});

describe("AccountCard", () => {
  it("shows the token's remaining life and never the whole client ID", () => {
    const view = account({
      phase: "ready",
      token: { obtained_at: NOW - 600, expires_at: NOW + 7200, drive: "scheduled" },
      last_served_at: NOW - 30,
    });
    render(<AccountCard account={view} now={NOW} automation busy={false} onLogin={() => {}} />);
    expect(screen.getByText("ready")).toBeInTheDocument();
    expect(screen.getByText("valid for 2h 0m")).toBeInTheDocument();
    expect(screen.getByText("30s ago")).toBeInTheDocument();
    expect(screen.getByText(/client ••••0001/)).toBeInTheDocument();
  });

  it("marks an expired token and offers the consent link after a failure", () => {
    const view = account({
      phase: "stopped_for_today",
      last_error: "browser: Dhan said: Too many attempts",
      consent_url: "https://auth.dhan.co/login/consentApp-login?consentAppId=c-1",
      token: { obtained_at: NOW - 90_000, expires_at: NOW - 10, drive: "manual" },
    });
    render(<AccountCard account={view} now={NOW} automation busy={false} onLogin={() => {}} />);
    expect(screen.getByText("stopped for today")).toBeInTheDocument();
    expect(screen.getByText("expired")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open the Dhan consent page" })).toHaveAttribute("rel", "noreferrer noopener");
    expect(screen.getByText(/Too many attempts/)).toBeInTheDocument();
  });

  it("asks for the right drive and disables what cannot run", async () => {
    const onLogin = vi.fn();
    const { rerender } = render(<AccountCard account={account()} now={NOW} automation busy={false} onLogin={onLogin} />);
    await userEvent.click(screen.getByRole("button", { name: "Run automated login" }));
    await userEvent.click(screen.getByRole("button", { name: "Manual login" }));
    expect(onLogin.mock.calls).toEqual([
      ["main", "automated"],
      ["main", "manual"],
    ]);

    rerender(<AccountCard account={account()} now={NOW} automation={false} busy={false} onLogin={onLogin} />);
    expect(screen.getByRole("button", { name: "Run automated login" })).toBeDisabled();
    rerender(<AccountCard account={account({ phase: "awaiting_callback" })} now={NOW} automation busy={false} onLogin={onLogin} />);
    expect(screen.getByRole("button", { name: "Manual login" })).toBeDisabled();
    expect(screen.getByText("No token stored for this account.")).toBeInTheDocument();
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
    const { rerender } = render(<EventLog events={[]} />);
    expect(screen.getByText("Nothing has happened yet.")).toBeInTheDocument();
    rerender(<EventLog events={[{ at: NOW, kind: "problem", alias: "main", message: "login failed" }]} />);
    expect(screen.getByText("login failed")).toBeInTheDocument();

    render(<RequestLog requests={[{ at: NOW, client_id: "1", alias: "main", ip: "203.0.113.7", outcome: "served", reason: "valid for 20h" }]} />);
    expect(screen.getByText("203.0.113.7")).toBeInTheDocument();
    expect(screen.getByText("served")).toHaveClass("tone-done");

    render(<AttemptLog attempts={[{ alias: "main", drive: "scheduled", started_at: NOW, finished_at: NOW + 42, outcome: "failed", stage: "browser", detail: "Invalid TOTP", consent_spent: true }]} />);
    expect(screen.getByText("failed at browser")).toHaveClass("tone-failed");
    expect(screen.getByText("42s")).toBeInTheDocument();
  });

  it("says when no engine or login has run", () => {
    render(<RequestLog requests={[]} />);
    render(<AttemptLog attempts={[]} />);
    expect(screen.getByText("No engine has asked for a token yet.")).toBeInTheDocument();
    expect(screen.getByText("No login has run yet.")).toBeInTheDocument();
  });
});
