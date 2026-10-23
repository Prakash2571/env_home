import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { addAccount, removeAccount, rotateTokenSeed } from "../src/api/client";
import { AccountsPage } from "../src/components/AccountsPage";
import { NOW, account } from "./support/factories";

const SEED = "KRSXG5CTMVRXEZLUKN2XAZLSKNSWG4TF";
const ADDED = { alias: "alt", token_api_seed: SEED, otpauth_uri: `otpauth://totp/BrokerLogin:alt?secret=${SEED}` };

function respond(status: number, body: unknown) {
  return Promise.resolve(new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
}

async function fillForm() {
  const user = userEvent.setup();
  await user.type(screen.getByLabelText("Alias"), "alt");
  await user.type(screen.getByLabelText("Client ID"), "1100000002");
  await user.type(screen.getByLabelText("API key"), "key-2");
  await user.type(screen.getByLabelText("API secret"), "secret-2");
  await user.type(screen.getByLabelText("Dhan login ID"), "8888888888");
  await user.type(screen.getByLabelText("PIN"), "654321");
  await user.type(screen.getByLabelText("Dhan TOTP secret"), "JBSWY3DPEHPK3PXP");
  return user;
}

describe("AccountsPage", () => {
  it("lists the accounts, or says there are none", () => {
    const { rerender } = render(<AccountsPage accounts={[account()]} loaded now={NOW} />);
    const list = screen.getByRole("table", { name: "Dhan accounts" });
    expect(within(list).getByText("main")).toBeInTheDocument();
    expect(within(list).getByText("••••0001")).toBeInTheDocument();

    rerender(<AccountsPage accounts={[]} loaded now={NOW} />);
    expect(screen.getByText("No Dhan account yet. Add the first one below.")).toBeInTheDocument();

    rerender(<AccountsPage accounts={[]} loaded={false} now={NOW} />);
    expect(screen.getByText(/still loading/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add account" })).toBeDisabled();
  });

  it("adds an account, shows its seed once, and forgets the form", async () => {
    const fetch = vi.spyOn(globalThis, "fetch").mockImplementation(() => respond(201, ADDED));
    render(<AccountsPage accounts={[]} loaded now={NOW} />);
    const user = await fillForm();
    await user.click(screen.getByRole("button", { name: "Add account" }));

    const [path, init] = fetch.mock.calls[0]!;
    expect(path).toBe("/api/accounts");
    expect(JSON.parse(String(init?.body))).toEqual({
      alias: "alt",
      client_id: "1100000002",
      api_key: "key-2",
      api_secret: "secret-2",
      account_id: "8888888888",
      pin: "654321",
      dhan_totp_secret: "JBSWY3DPEHPK3PXP",
    });

    const reveal = await screen.findByRole("region", { name: "token API seed for alt" });
    expect(reveal).toHaveTextContent(SEED);
    expect(reveal).toHaveTextContent("shown once");
    expect(screen.getByLabelText("API secret")).toHaveValue("");
    expect(screen.getByLabelText("PIN")).toHaveValue("");

    await user.click(within(reveal).getByRole("button", { name: "I have stored it" }));
    expect(screen.queryByText(SEED)).toBeNull();
  });

  it("shows the server's problems next to their fields", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() =>
      respond(400, { error: "some fields need fixing", problems: [{ field: "pin", message: "4 to 8 digits" }] }),
    );
    render(<AccountsPage accounts={[]} loaded now={NOW} />);
    const user = await fillForm();
    await user.click(screen.getByRole("button", { name: "Add account" }));
    expect(await screen.findByText("4 to 8 digits")).toBeInTheDocument();
    expect(screen.getByLabelText("PIN")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByRole("alert")).toHaveTextContent("some fields need fixing");
    expect(screen.getByLabelText("Alias"), "the form is kept to be fixed").toHaveValue("alt");
  });

  it("removes an account only after its alias is typed", async () => {
    const fetch = vi.spyOn(globalThis, "fetch").mockImplementation(() => respond(200, { removed: "main" }));
    render(<AccountsPage accounts={[account()]} loaded now={NOW} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Remove main" }));
    const confirm = screen.getByRole("button", { name: "Remove for good" });
    expect(confirm).toBeDisabled();
    await user.type(screen.getByLabelText("Type main to confirm"), "mai");
    expect(confirm).toBeDisabled();
    await user.type(screen.getByLabelText("Type main to confirm"), "n");
    await user.click(confirm);
    expect(fetch).toHaveBeenCalledWith(
      "/api/accounts/main",
      expect.objectContaining({ method: "DELETE", body: '{"confirm":"main"}' }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent("main removed");
  });

  it("replaces a token API seed and shows the new one once", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => respond(200, { ...ADDED, alias: "main" }));
    render(<AccountsPage accounts={[account()]} loaded now={NOW} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "New token seed for main" }));
    await user.type(screen.getByLabelText("Type main to confirm"), "main");
    await user.click(screen.getByRole("button", { name: "Replace the seed" }));
    expect(await screen.findByRole("region", { name: "token API seed for main" })).toHaveTextContent(SEED);

    await user.click(screen.getByRole("button", { name: "New token seed for main" }));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByLabelText("Type main to confirm")).toBeNull();
  });
});

describe("account requests", () => {
  it("send JSON and carry the server's field problems on failure", async () => {
    const fetch = vi
      .spyOn(globalThis, "fetch")
      .mockImplementationOnce(() => respond(201, ADDED))
      .mockImplementationOnce(() => respond(409, { error: "that alias is taken" }))
      .mockImplementationOnce(() => respond(200, ADDED))
      .mockImplementationOnce(() => Promise.resolve(new Response("oops", { status: 500 })));
    const form = {
      alias: "a",
      client_id: "1",
      api_key: "k",
      api_secret: "s",
      account_id: "i",
      pin: "1234",
      dhan_totp_secret: "JBSWY3DPEHPK3PXP",
    };
    await expect(addAccount(form)).resolves.toEqual(ADDED);
    await expect(addAccount(form)).rejects.toMatchObject({ status: 409, message: "that alias is taken", problems: [] });
    await expect(rotateTokenSeed("a b", "a b")).resolves.toEqual(ADDED);
    expect(fetch.mock.calls[2]![0]).toBe("/api/accounts/a%20b/token-seed");
    await expect(removeAccount("a", "a")).rejects.toMatchObject({ status: 500, message: "HTTP 500" });
  });
});
