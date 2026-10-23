import { useEffect, useState } from "react";
import type { AddedAccount } from "../api/types";

interface SeedRevealProps {
  added: AddedAccount;
  onDone: () => void;
}

export function SeedReveal({ added, onDone }: SeedRevealProps) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    // The seed cannot be shown again: closing or reloading the tab asks first.
    const ask = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", ask);
    return () => window.removeEventListener("beforeunload", ask);
  }, []);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(added.token_api_seed);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return (
    <section className="panel reveal" aria-label={`token API seed for ${added.alias}`}>
      <h3>Token API seed for {added.alias}</h3>
      <p>
        This is shown once. Put it in the trading engine's .env now: the engine makes its TOTP codes from it. The
        service never shows it again; if it is lost, replace it.
      </p>
      <p className="seed mono">{added.token_api_seed}</p>
      <details>
        <summary className="muted">otpauth URI</summary>
        <p className="mono seed-uri">{added.otpauth_uri}</p>
      </details>
      <div className="actions">
        <button type="button" className="action" onClick={copy}>
          {copied ? "Copied" : "Copy the seed"}
        </button>
        <button type="button" className="action primary" onClick={onDone}>
          I have stored it
        </button>
      </div>
    </section>
  );
}
