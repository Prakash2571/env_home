import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { App } from "../src/App";
import { ApiError, fetchSnapshot, requestLogin, testTelegram } from "../src/api/client";
import { ago, duration, istClock, istDate } from "../src/format";
import { NOW, snapshot } from "./support/factories";
import { StubEventSource } from "./support/setup";

function respond(status: number, body: unknown) {
  return Promise.resolve(new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
}

describe("App", () => {
  it("renders the first snapshot and follows the stream", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => respond(200, snapshot()));
    render(<App />);
    expect(await screen.findByRole("article", { name: "account main" })).toBeInTheDocument();

    const stream = StubEventSource.latest();
    expect(stream.url).toBe("/api/stream");
    act(() => {
      stream.emitOpen();
      stream.emit("snapshot", JSON.stringify(snapshot({ accounts: [], events: [{ at: NOW, kind: "login", alias: null, message: "morning summary" }] })));
    });
    expect(await screen.findByText("morning summary")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "service status" })).toHaveTextContent("live");

    act(() => stream.emit("snapshot", "{not json"));
    expect(await screen.findByText("morning summary")).toBeInTheDocument();
    act(() => stream.emitError());
    expect(screen.getByRole("list", { name: "service status" })).toHaveTextContent("reconnecting");
  });

  it("lists configuration problems when the service is not configured", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() =>
      respond(200, snapshot({ configured: false, problems: ["CBL_ENCRYPTION_KEY is missing"], accounts: [] })),
    );
    render(<App />);
    expect(await screen.findByText("CBL_ENCRYPTION_KEY is missing")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Log in every account" })).toBeNull();
  });

  it("switches between logins and accounts, and remembers it in the address", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => respond(200, snapshot({ accounts: [] })));
    window.history.replaceState(null, "", "/");
    render(<App />);
    const user = userEvent.setup();
    expect(await screen.findByText("No Dhan account yet.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Add an account" }));
    expect(screen.getByRole("form", { name: "add a Dhan account" })).toBeInTheDocument();
    expect(window.location.hash).toBe("#accounts");
    expect(screen.getByRole("button", { name: "Accounts" })).toHaveAttribute("aria-current", "page");

    await user.click(screen.getByRole("button", { name: "Logins" }));
    expect(window.location.hash).toBe("");
    expect(screen.queryByRole("form", { name: "add a Dhan account" })).toBeNull();

    act(() => {
      window.location.hash = "#accounts";
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    });
    expect(screen.getByRole("form", { name: "add a Dhan account" })).toBeInTheDocument();
    window.history.replaceState(null, "", "/");
  });

  it("asks the visitor to sign in when Access refuses", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => respond(401, { error: "sign in through Cloudflare Access" }));
    render(<App />);
    expect(await screen.findByText("Sign in through Cloudflare Access to see this page.")).toBeInTheDocument();
  });

  it("queues logins and reports what the server said", async () => {
    const fetch = vi.spyOn(globalThis, "fetch").mockImplementation((input) =>
      String(input) === "/api/state" ? respond(200, snapshot()) : respond(409, { error: "there is no browser sidecar; use a manual login" }),
    );
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Log in every account" }));
    expect(await screen.findByRole("status")).toHaveTextContent("there is no browser sidecar");
    expect(fetch).toHaveBeenCalledWith("/api/accounts/login", expect.objectContaining({ method: "POST", body: '{"drive":"automated"}' }));

    fetch.mockImplementation(() => respond(202, { queued: true }));
    await userEvent.click(screen.getByRole("button", { name: "Test Telegram" }));
    expect(await screen.findByRole("status")).toHaveTextContent("test notice queued");
  });
});

describe("client", () => {
  it("encodes aliases and surfaces server errors", async () => {
    const fetch = vi.spyOn(globalThis, "fetch").mockImplementation(() => respond(202, {}));
    await requestLogin("a/b", "manual");
    expect(fetch).toHaveBeenCalledWith("/api/accounts/a%2Fb/login", expect.anything());
    await testTelegram();

    fetch.mockImplementation(() => Promise.resolve(new Response("oops", { status: 502 })));
    await expect(fetchSnapshot()).rejects.toEqual(new ApiError(502, "HTTP 502"));
    fetch.mockImplementation(() => respond(500, { other: 1 }));
    await expect(requestLogin(null, "automated")).rejects.toThrow("HTTP 500");
  });
});

describe("format", () => {
  it("reads durations and IST times", () => {
    expect(duration(5)).toBe("5s");
    expect(duration(125)).toBe("2m 5s");
    expect(duration(7260)).toBe("2h 1m");
    expect(duration(-3)).toBe("0s");
    expect(istClock(1_792_656_000)).toBe("13:30:00");
    expect(istDate(1_792_656_000)).toBe("2026-10-22");
    expect(ago(NOW - 2, NOW)).toBe("just now");
    expect(ago(NOW - 90, NOW)).toBe("1m 30s ago");
  });
});

describe("stream lifecycle", () => {
  it("closes the stream when the page goes away", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => respond(200, snapshot()));
    const { unmount } = render(<App />);
    await waitFor(() => expect(StubEventSource.instances).toHaveLength(1));
    unmount();
    expect(StubEventSource.latest().closed).toBe(true);
  });
});
