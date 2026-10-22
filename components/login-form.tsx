"use client";

import { FormEvent, useState } from "react";

export default function LoginForm() {
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const response = await fetch("/api/auth/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ password }),
      });
      if (!response.ok) {
        const data = await response.json();
        throw new Error(data.error || "Could not sign in");
      }
      window.location.assign("/");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not sign in");
      setBusy(false);
    }
  }

  return <main className="login-shell">
    <div className="login-card">
      <div className="brand-mark">E</div>
      <p className="eyebrow">PRIVATE WORKSPACE</p>
      <h1>Welcome to Env Home</h1>
      <p className="muted">Your environment variables, organized in one place.</p>
      <form onSubmit={submit}>
        <label htmlFor="password">Admin password</label>
        <input id="password" type="password" autoComplete="current-password" required
          value={password} onChange={(event) => setPassword(event.target.value)} />
        {error && <p className="error" role="alert">{error}</p>}
        <button className="button primary full" disabled={busy}>{busy ? "Signing in…" : "Sign in"}</button>
      </form>
    </div>
  </main>;
}
