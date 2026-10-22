import type { Link } from "../api/useSnapshot";
import type { Snapshot } from "../api/types";
import { duration } from "../format";

interface StatusStripProps {
  snapshot: Snapshot;
  link: Link;
}

const LINK_LABEL: Record<Link, string> = {
  connecting: "connecting",
  live: "live",
  reconnecting: "reconnecting",
  denied: "sign-in required",
};

export function StatusStrip({ snapshot, link }: StatusStripProps) {
  const loginWindow = snapshot.window;
  const items: { label: string; value: string; tone: string }[] = [
    { label: "stream", value: LINK_LABEL[link], tone: link === "live" ? "done" : "warn" },
    {
      label: "login window",
      value:
        loginWindow === null
          ? "—"
          : loginWindow.open
            ? `${loginWindow.label} · open, ${duration(loginWindow.closes_in)} left`
            : `${loginWindow.label} · opens in ${duration(loginWindow.opens_in)}`,
      tone: loginWindow?.open ? "running" : "dim",
    },
    {
      label: "store",
      value: snapshot.store === null ? "—" : `${snapshot.store}${snapshot.store_reachable ? "" : " (unreachable)"}`,
      tone: snapshot.store_reachable ? "done" : "failed",
    },
    {
      label: "browser sidecar",
      value: snapshot.automation ? "configured" : "manual only",
      tone: snapshot.automation ? "done" : "warn",
    },
    {
      label: "telegram",
      value: snapshot.telegram.configured
        ? snapshot.telegram.last_error ?? `${snapshot.telegram.delivered} delivered`
        : "not configured",
      tone: !snapshot.telegram.configured ? "dim" : snapshot.telegram.last_error ? "failed" : "done",
    },
    {
      label: "token API",
      value: snapshot.api_requires_access_jwt ? "Access header + TOTP" : "client ID + TOTP",
      tone: "dim",
    },
  ];
  return (
    <ul className="strip" aria-label="service status">
      {items.map((item) => (
        <li key={item.label}>
          <span className="strip-label">{item.label}</span>
          <span className={`strip-value tone-${item.tone}`}>{item.value}</span>
        </li>
      ))}
    </ul>
  );
}
