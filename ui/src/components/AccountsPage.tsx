import { useState } from "react";
import { removeAccount, rotateTokenSeed } from "../api/client";
import type { AccountView, AddedAccount } from "../api/types";
import { PHASE } from "./phase";
import { AddAccountForm } from "./AddAccountForm";
import { SeedReveal } from "./SeedReveal";
import { TokenLife } from "./TokenLife";

type Change = "remove" | "rotate";

interface Pending {
  alias: string;
  change: Change;
}

const CHANGE: Record<Change, { confirm: string; explain: string }> = {
  remove: {
    confirm: "Remove for good",
    explain:
      "Deletes its secrets and stored tokens. Engines asking for it get 401. Login history stays.",
  },
  rotate: {
    confirm: "Replace the seed",
    explain:
      "Engines using the current seed get 401 until they are given the new one.",
  },
};

interface ConfirmRowProps {
  pending: Pending;
  working: boolean;
  onConfirm: (typed: string) => void;
  onCancel: () => void;
}

/** Aliases are lower case, so capitals or stray spaces from a keyboard still match. */
function normalise(typed: string): string {
  return typed.trim().toLowerCase();
}

function ConfirmRow({ pending, working, onConfirm, onCancel }: ConfirmRowProps) {
  const [typed, setTyped] = useState("");
  const change = CHANGE[pending.change];
  const id = `confirm-${pending.alias}`;
  const matches = normalise(typed) === pending.alias;
  return (
    <tr className="confirm-row">
      <td colSpan={5}>
        <p className="muted">{change.explain}</p>
        <div className="confirm">
          <label htmlFor={id}>
            Type <code className="confirm-alias">{pending.alias}</code> to confirm
          </label>
          <input
            id={id}
            className="mono"
            autoComplete="off"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            value={typed}
            aria-describedby={`${id}-hint`}
            onChange={(event) => setTyped(event.target.value)}
          />
          <button
            type="button"
            className={pending.change === "remove" ? "action danger" : "action primary"}
            disabled={working || !matches}
            onClick={() => onConfirm(normalise(typed))}
          >
            {change.confirm}
          </button>
          <button type="button" className="action" onClick={onCancel}>
            Cancel
          </button>
        </div>
        <p id={`${id}-hint`} className="field-note muted">
          {typed === "" || matches ? "\u00a0" : `type the alias exactly: ${pending.alias}`}
        </p>
      </td>
    </tr>
  );
}

interface AccountsPageProps {
  accounts: AccountView[];
  loaded: boolean;
  now: number;
}

export function AccountsPage({ accounts, loaded, now }: AccountsPageProps) {
  const [shown, setShown] = useState<AddedAccount | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [working, setWorking] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const confirm = async (typed: string) => {
    if (pending === null) {
      return;
    }
    setWorking(true);
    setStatus(null);
    try {
      if (pending.change === "remove") {
        await removeAccount(pending.alias, typed);
        setStatus(`${pending.alias} removed`);
      } else {
        setShown(await rotateTokenSeed(pending.alias, typed));
        setStatus(`new token API seed for ${pending.alias}`);
      }
      setPending(null);
    } catch (cause) {
      setStatus(cause instanceof Error ? cause.message : "the change failed");
    } finally {
      setWorking(false);
    }
  };

  return (
    <section className="accounts-page" aria-label="manage accounts">
      {shown === null ? null : (
        <SeedReveal added={shown} onDone={() => setShown(null)} />
      )}
      {status === null ? null : (
        <p className="notice" role="status">
          {status}
        </p>
      )}

      <section className="panel">
        <h3>Dhan accounts</h3>
        {!loaded ? (
          <p className="muted">
            The stored accounts are still loading; changes wait until they are
            in.
          </p>
        ) : accounts.length === 0 ? (
          <p className="muted">No Dhan account yet. Add the first one below.</p>
        ) : (
          <div className="table-wrap">
            <table className="table accounts-table" aria-label="Dhan accounts">
              <thead>
                <tr>
                  <th>account</th>
                  <th>client</th>
                  <th>status</th>
                  <th>token</th>
                  <th aria-label="changes" />
                </tr>
              </thead>
              <tbody>
                {accounts.map((account) => {
                  const phase = PHASE[account.phase];
                  const open =
                    pending?.alias === account.alias ? pending : null;
                  return [
                    <tr key={account.alias}>
                      <td className="mono">{account.alias}</td>
                      <td className="mono">{account.client_id_hint}</td>
                      <td className={`tone-${phase.tone}`}>{phase.label}</td>
                      <td className="token-cell">
                      <TokenLife token={account.token} now={now} />
                    </td>
                      <td className="row-actions">
                        <button
                          type="button"
                          className="action"
                          aria-label={`New token seed for ${account.alias}`}
                          onClick={() =>
                            setPending({
                              alias: account.alias,
                              change: "rotate",
                            })
                          }
                        >
                          New token seed
                        </button>
                        <button
                          type="button"
                          className="action"
                          aria-label={`Remove ${account.alias}`}
                          onClick={() =>
                            setPending({
                              alias: account.alias,
                              change: "remove",
                            })
                          }
                        >
                          Remove
                        </button>
                      </td>
                    </tr>,
                    open === null ? null : (
                      <ConfirmRow
                        key={`${account.alias}-${open.change}`}
                        pending={open}
                        working={working}
                        onConfirm={confirm}
                        onCancel={() => setPending(null)}
                      />
                    ),
                  ];
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <AddAccountForm disabled={!loaded} onAdded={setShown} />
    </section>
  );
}
