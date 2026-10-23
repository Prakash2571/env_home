import { type FormEvent, useState } from "react";
import { ApiError, addAccount } from "../api/client";
import type { AccountField, AccountForm, AddedAccount } from "../api/types";

interface FieldSpec {
  name: AccountField;
  label: string;
  hint: string;
  secret: boolean;
}

const FIELDS: FieldSpec[] = [
  { name: "alias", label: "Alias", hint: "a short name: lower-case letters, digits or _", secret: false },
  { name: "client_id", label: "Client ID", hint: "the Dhan client ID trading engines ask for", secret: false },
  { name: "api_key", label: "API key", hint: "from the account's DhanHQ API app", secret: false },
  { name: "api_secret", label: "API secret", hint: "from the same API app", secret: true },
  { name: "account_id", label: "Dhan login ID", hint: "what Dhan's login page asks for", secret: false },
  { name: "pin", label: "PIN", hint: "the account's login PIN", secret: true },
  {
    name: "dhan_totp_secret",
    label: "Dhan TOTP secret",
    hint: "the base32 key Dhan showed when you turned on TOTP",
    secret: true,
  },
];

const EMPTY: AccountForm = {
  alias: "",
  client_id: "",
  api_key: "",
  api_secret: "",
  account_id: "",
  pin: "",
  dhan_totp_secret: "",
};

interface AddAccountFormProps {
  disabled: boolean;
  onAdded: (added: AddedAccount) => void;
}

type Problems = Partial<Record<AccountField, string>>;

export function AddAccountForm({ disabled, onAdded }: AddAccountFormProps) {
  const [form, setForm] = useState<AccountForm>(EMPTY);
  const [problems, setProblems] = useState<Problems>({});
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSending(true);
    setError(null);
    setProblems({});
    try {
      const added = await addAccount(form);
      setForm(EMPTY);
      onAdded(added);
    } catch (cause) {
      if (cause instanceof ApiError) {
        setProblems(Object.fromEntries(cause.problems.map((problem) => [problem.field, problem.message])));
      }
      setError(cause instanceof Error ? cause.message : "the account could not be added");
    } finally {
      setSending(false);
    }
  };

  return (
    <form className="panel account-form" aria-label="add a Dhan account" onSubmit={submit} noValidate>
      <h3>Add a Dhan account</h3>
      <p className="muted">
        Everything here is encrypted before it is stored and never shown again. The token API seed for the trading
        engine is generated for you.
      </p>
      <div className="fields">
        {FIELDS.map((field) => {
          const id = `account-${field.name}`;
          const problem = problems[field.name];
          return (
            <div key={field.name} className="field">
              <label htmlFor={id}>{field.label}</label>
              <input
                id={id}
                name={field.name}
                type={field.secret ? "password" : "text"}
                className="mono"
                autoComplete="off"
                spellCheck={false}
                value={form[field.name]}
                aria-invalid={problem === undefined ? undefined : true}
                aria-describedby={`${id}-note`}
                onChange={(event) => setForm({ ...form, [field.name]: event.target.value })}
              />
              <span id={`${id}-note`} className={problem === undefined ? "field-note muted" : "field-note tone-failed"}>
                {problem ?? field.hint}
              </span>
            </div>
          );
        })}
      </div>
      {error === null ? null : (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="actions">
        <button type="submit" className="action primary" disabled={disabled || sending}>
          Add account
        </button>
      </div>
    </form>
  );
}
