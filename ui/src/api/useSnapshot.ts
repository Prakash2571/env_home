import { useEffect, useState } from "react";
import { fetchSnapshot } from "./client";
import type { Snapshot } from "./types";

export type Link = "connecting" | "live" | "reconnecting" | "denied";

export interface SnapshotState {
  snapshot: Snapshot | null;
  link: Link;
  error: string | null;
}

export function useSnapshot(): SnapshotState {
  const [state, setState] = useState<SnapshotState>({ snapshot: null, link: "connecting", error: null });

  useEffect(() => {
    let cancelled = false;

    fetchSnapshot()
      .then((snapshot) => {
        if (!cancelled) {
          setState((current) => ({ ...current, snapshot, error: null }));
        }
      })
      .catch((cause: { status?: number; message?: string }) => {
        if (!cancelled) {
          setState((current) => ({
            ...current,
            link: cause.status === 401 || cause.status === 403 ? "denied" : current.link,
            error: cause.message ?? "could not load the state",
          }));
        }
      });

    const source = new EventSource("/api/stream");
    source.onopen = () => {
      if (!cancelled) {
        setState((current) => ({ ...current, link: "live" }));
      }
    };
    source.addEventListener("snapshot", (event) => {
      if (cancelled) {
        return;
      }
      try {
        const snapshot = JSON.parse((event as MessageEvent<string>).data) as Snapshot;
        setState({ snapshot, link: "live", error: null });
      } catch {
        setState((current) => ({ ...current, error: "the stream sent an unreadable snapshot" }));
      }
    });
    source.onerror = () => {
      if (!cancelled) {
        setState((current) => ({ ...current, link: current.link === "denied" ? "denied" : "reconnecting" }));
      }
    };

    return () => {
      cancelled = true;
      source.close();
    };
  }, []);

  return state;
}
