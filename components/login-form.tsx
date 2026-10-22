"use client";

import { FormEvent, useState } from "react";

export default function LoginForm() {
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
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
    <div className="login-glow glow-one" aria-hidden="true" />
    <div className="login-glow glow-two" aria-hidden="true" />
    <div className="login-layout">
      <section className="login-intro" aria-label="About Env Home">
        <div className="login-brand">
          <span className="brand-mark">E</span>
          <span>env<span className="brand-muted">home</span></span>
        </div>

        <div className="login-copy">
          <p className="login-kicker"><span className="status-dot" /> YOUR PRIVATE CONFIGURATION HUB</p>
          <h1>Every environment.<br /><span>One secure home.</span></h1>
          <p>Keep your variables organized, update them in seconds, and give each server exactly the access it needs.</p>
        </div>

        <div className="preview-window" aria-hidden="true">
          <div className="preview-toolbar"><span className="preview-dots"><i /><i /><i /></span><span>production / api</span><span className="preview-live"><i /> LIVE</span></div>
          <div className="preview-code">
            <div><span className="line-number">01</span><span className="code-comment"># One place for your configuration</span></div>
            <div><span className="line-number">02</span><span className="code-key">DATABASE_URL</span><span className="code-equal">=</span><span className="code-mask">••••••••••••••••</span></div>
            <div><span className="line-number">03</span><span className="code-key">API_KEY</span><span className="code-equal">=</span><span className="code-mask">••••••••••••••••</span></div>
            <div><span className="line-number">04</span><span className="code-key">APP_MODE</span><span className="code-equal">=</span><span className="code-value">production</span></div>
          </div>
          <div className="preview-footer"><span className="preview-lock">◆</span> Encrypted at rest <span className="preview-divider">·</span> Ready for your servers</div>
        </div>

        <div className="login-benefits"><span><i>✓</i> Self-hosted</span><span><i>✓</i> Grouped variables</span><span><i>✓</i> Scoped access keys</span></div>
      </section>

      <section className="login-panel" aria-labelledby="login-heading">
        <div className="login-card">
          <div className="login-card-icon" aria-hidden="true">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><rect x="5" y="10" width="14" height="11" rx="2" /><path d="M8 10V7a4 4 0 0 1 8 0v3" /><path d="M12 14v3" /></svg>
          </div>
          <p className="eyebrow">ADMIN ACCESS</p>
          <h2 id="login-heading">Welcome back</h2>
          <p className="login-subtitle">Enter your admin password to open your workspace.</p>
          <form onSubmit={submit}>
            <label htmlFor="password">Admin password</label>
            <div className="password-field">
              <input id="password" type={showPassword ? "text" : "password"} autoComplete="current-password"
                placeholder="Enter your password" required autoFocus value={password}
                onChange={(event) => setPassword(event.target.value)} />
              <button type="button" className="password-toggle" onClick={() => setShowPassword(!showPassword)}
                aria-label={showPassword ? "Hide password" : "Show password"}>{showPassword ? "Hide" : "Show"}</button>
            </div>
            {error && <p className="error" role="alert">{error}</p>}
            <button className="button primary full login-submit" disabled={busy}>
              {busy ? "Signing in…" : "Sign in to Env Home"} <span aria-hidden="true">→</span>
            </button>
          </form>
          <div className="login-card-footer"><span className="status-dot" /> Your workspace, your server.</div>
        </div>
      </section>
    </div>
  </main>;
}
