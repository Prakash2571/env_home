"use client";

import { useCallback, useEffect, useState } from "react";

type Group = {
  id: string; name: string; slug: string; allowed_ips: string[];
  secret_count?: number; token_count?: number;
};
type Pair = { name: string; value: string };
type Access = { id: string; label: string; created_at: string; last_used_at: string | null };
type Detail = { group: Group; secrets: Pair[]; tokens: Access[] };

async function api<T>(path: string, method = "GET", body?: unknown): Promise<T> {
  const response = await fetch(path, {
    method,
    headers: body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    cache: "no-store",
  });
  const data = await response.json();
  if (response.status === 401) {
    window.location.assign("/login");
    throw new Error("Session expired");
  }
  if (!response.ok) throw new Error(data.error || "Request failed");
  return data as T;
}

function parsePaste(text: string): Pair[] {
  const pairs: Pair[] = [];
  for (const [index, line] of text.split(/\r?\n/).entries()) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const source = trimmed.startsWith("export ") ? trimmed.slice(7) : line;
    const equals = source.indexOf("=");
    const name = source.slice(0, equals).trim();
    if (equals < 0 || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
      throw new Error(`Invalid variable on line ${index + 1}`);
    }
    let value = source.slice(equals + 1);
    if (value.length >= 2 && ((value[0] === '"' && value.at(-1) === '"') ||
      (value[0] === "'" && value.at(-1) === "'"))) value = value.slice(1, -1);
    pairs.push({ name, value });
  }
  return pairs;
}

export default function Dashboard() {
  const [groups, setGroups] = useState<Group[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [pairs, setPairs] = useState<Pair[]>([]);
  const [name, setName] = useState("");
  const [ips, setIps] = useState("");
  const [paste, setPaste] = useState("");
  const [newGroup, setNewGroup] = useState("");
  const [keyLabel, setKeyLabel] = useState("");
  const [newKey, setNewKey] = useState("");
  const [showValues, setShowValues] = useState(false);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);

  const refreshList = useCallback(async () => {
    const data = await api<{ groups: Group[] }>("/api/admin/groups");
    setGroups(data.groups);
    return data.groups;
  }, []);

  const loadGroup = useCallback(async (id: string) => {
    const data = await api<Detail>(`/api/admin/groups/${id}`);
    setDetail(data);
    setPairs(data.secrets);
    setName(data.group.name);
    setIps(data.group.allowed_ips.join("\n"));
    setNewKey("");
    setShowValues(false);
  }, []);

  useEffect(() => {
    refreshList().then((list) => { if (list[0]) setSelected(list[0].id); })
      .catch((error) => setMessage(error.message));
  }, [refreshList]);

  useEffect(() => {
    if (!selected) { setDetail(null); return; }
    loadGroup(selected).catch((error) => setMessage(error.message));
  }, [selected, loadGroup]);

  async function action(work: () => Promise<void>, success: string) {
    setBusy(true);
    setMessage("");
    try { await work(); setMessage(success); }
    catch (error) { setMessage(error instanceof Error ? error.message : "Something went wrong"); }
    finally { setBusy(false); }
  }

  function updatePair(index: number, key: keyof Pair, value: string) {
    setPairs((current) => current.map((pair, i) => i === index ? { ...pair, [key]: value } : pair));
  }

  return <div className="app-shell">
    <aside className="sidebar">
      <div className="brand"><span className="brand-mark small">E</span><span>Env Home</span></div>
      <div className="sidebar-label">GROUPS <span>{groups.length}</span></div>
      <nav aria-label="Groups" className="group-list">
        {groups.map((group) => <button key={group.id}
          className={`group-item ${selected === group.id ? "active" : ""}`}
          onClick={() => { setSelected(group.id); setMessage(""); }}>
          <span className="group-icon">{group.name[0].toUpperCase()}</span>
          <span className="group-text"><strong>{group.name}</strong><small>{group.secret_count ?? 0} variables</small></span>
        </button>)}
      </nav>
      <form className="new-group" onSubmit={(event) => {
        event.preventDefault();
        action(async () => {
          const data = await api<{ group: Group }>("/api/admin/groups", "POST", { name: newGroup });
          setNewGroup("");
          await refreshList();
          setSelected(data.group.id);
        }, "Group created");
      }}>
        <input aria-label="New group name" placeholder="New group name" required maxLength={80}
          value={newGroup} onChange={(event) => setNewGroup(event.target.value)} />
        <button className="button secondary" disabled={busy || !newGroup.trim()}>Add group</button>
      </form>
      <button className="signout" onClick={() => {
        action(async () => {
          await api("/api/auth/logout", "POST");
          window.location.assign("/login");
        }, "");
      }}>Sign out</button>
    </aside>

    <main className="main">
      <header className="topbar"><div><p className="eyebrow">YOUR WORKSPACE</p><h1>{detail?.group.name ?? "Environment groups"}</h1>
        <p className="muted">{detail ? "Edit variables and control server access." : "Create a group to get started."}</p></div>
        {detail && <span className="slug-badge">/{detail.group.slug}</span>}
      </header>
      {message && <div className="notice" role="status">{message}</div>}
      {!detail ? <div className="empty">Create your first group from the sidebar.</div> : <>
        <section className="card">
          <div className="section-title"><div><h2>Variables</h2><p>Save changes to update the values your servers can fetch.</p></div>
            <button className="button secondary" onClick={() => setShowValues(!showValues)}>
              {showValues ? "Hide values" : "Show values"}</button></div>
          <div className="variable-list">
            {pairs.map((pair, index) => <div className="variable-row" key={index}>
              <input aria-label={`Variable name ${index + 1}`} placeholder="VARIABLE_NAME"
                value={pair.name} onChange={(event) => updatePair(index, "name", event.target.value)} />
              <input aria-label={`Value for ${pair.name || "variable"}`} placeholder="Value"
                type={showValues ? "text" : "password"} value={pair.value}
                onChange={(event) => updatePair(index, "value", event.target.value)} />
              <button className="icon-button" title="Remove variable" aria-label={`Remove ${pair.name}`}
                onClick={() => setPairs((current) => current.filter((_, i) => i !== index))}>×</button>
            </div>)}
            {pairs.length === 0 && <p className="muted empty-inline">No variables yet.</p>}
          </div>
          <div className="row-actions">
            <button className="button secondary" onClick={() => setPairs((current) => [...current, { name: "", value: "" }])}>+ Add variable</button>
            <button className="button primary" disabled={busy} onClick={() => action(async () => {
              await api(`/api/admin/groups/${selected}/secrets`, "PUT", { secrets: pairs });
              await loadGroup(selected!);
              await refreshList();
            }, "Variables saved")}>Save variables</button>
          </div>
        </section>

        <section className="card">
          <div className="section-title"><div><h2>Paste multiple variables</h2>
            <p>One <code>NAME=value</code> pair per line. Existing names are updated after you save.</p></div></div>
          <textarea value={paste} onChange={(event) => setPaste(event.target.value)}
            placeholder={"DATABASE_URL=postgres://...\nAPI_KEY=..."} rows={5} spellCheck={false} />
          <div className="row-actions end"><button className="button secondary" disabled={!paste.trim()} onClick={() => {
            try {
              const incoming = parsePaste(paste);
              setPairs((current) => {
                const merged = new Map(current.filter((pair) => pair.name).map((pair) => [pair.name, pair]));
                for (const pair of incoming) merged.set(pair.name, pair);
                return [...merged.values()];
              });
              setPaste("");
              setMessage(`${incoming.length} variable(s) added to the editor. Click Save variables to store them.`);
            } catch (error) { setMessage(error instanceof Error ? error.message : "Invalid input"); }
          }}>Add to editor</button></div>
        </section>

        <section className="card">
          <div className="section-title"><div><h2>Server access</h2>
            <p>Create a read key for each server. The key appears only once.</p></div></div>
          <div className="inline-form">
            <input aria-label="Access key label" placeholder="Server name, e.g. api-prod" maxLength={80}
              value={keyLabel} onChange={(event) => setKeyLabel(event.target.value)} />
            <button className="button primary" disabled={busy || !keyLabel.trim()} onClick={() => action(async () => {
              const data = await api<{ token: string }>(`/api/admin/groups/${selected}/tokens`, "POST", { label: keyLabel });
              setKeyLabel("");
              await loadGroup(selected!);
              setNewKey(data.token);
              await refreshList();
            }, "Access key created")}>Create key</button>
          </div>
          {newKey && <div className="key-reveal"><p>Copy this key now. It cannot be shown again.</p>
            <code>{newKey}</code>
            <button className="button secondary" onClick={() => navigator.clipboard.writeText(newKey)}>Copy key</button></div>}
          <div className="token-list">
            {detail.tokens.map((token) => <div className="token-row" key={token.id}>
              <div><strong>{token.label}</strong><small>Created {new Date(token.created_at).toLocaleDateString()}
                {token.last_used_at ? ` · Last used ${new Date(token.last_used_at).toLocaleString()}` : " · Never used"}</small></div>
              <button className="text-danger" disabled={busy} onClick={() => {
                if (!window.confirm(`Revoke ${token.label}?`)) return;
                action(async () => {
                  await api(`/api/admin/groups/${selected}/tokens/${token.id}`, "DELETE");
                  await loadGroup(selected!);
                  await refreshList();
                }, "Access key revoked");
              }}>Revoke</button>
            </div>)}
            {detail.tokens.length === 0 && <p className="muted empty-inline">No access keys yet.</p>}
          </div>
          <div className="api-example"><span>FETCH ENDPOINT</span><code>GET /api/v1/groups/{detail.group.slug}</code>
            <p>Send the key as <code>Authorization: Bearer YOUR_KEY</code>.</p></div>
        </section>

        <section className="card">
          <div className="section-title"><div><h2>Group settings</h2>
            <p>Optional IP rules restrict access keys to specific server addresses.</p></div></div>
          <div className="settings-grid">
            <div><label htmlFor="group-name">Group name</label>
              <input id="group-name" value={name} maxLength={80} onChange={(event) => setName(event.target.value)} /></div>
            <div><label htmlFor="allowed-ips">Allowed IPs or CIDR ranges, one per line</label>
              <textarea id="allowed-ips" rows={3} placeholder={"203.0.113.10\n2001:db8::/32"}
                value={ips} onChange={(event) => setIps(event.target.value)} />
              <small>Empty means any IP with a valid key. IP rules require a trusted proxy configured with X-Real-IP.</small></div>
          </div>
          <div className="row-actions">
            <button className="text-danger" disabled={busy} onClick={() => {
              if (!window.confirm(`Delete ${detail.group.name} and all its variables and keys?`)) return;
              action(async () => {
                await api(`/api/admin/groups/${selected}`, "DELETE");
                const list = await refreshList();
                setSelected(list[0]?.id ?? null);
              }, "Group deleted");
            }}>Delete group</button>
            <button className="button primary" disabled={busy} onClick={() => action(async () => {
              await api(`/api/admin/groups/${selected}`, "PATCH", {
                name, allowed_ips: ips.split(/\r?\n/).map((line) => line.trim()).filter(Boolean)
              });
              await loadGroup(selected!);
              await refreshList();
            }, "Settings saved")}>Save settings</button>
          </div>
        </section>
      </>}
    </main>
  </div>;
}
