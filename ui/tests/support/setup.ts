import "@testing-library/jest-dom/vitest";
import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";

type Listener = (event: MessageEvent<string>) => void;

export class StubEventSource {
  static instances: StubEventSource[] = [];

  url: string;
  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  closed = false;
  private listeners = new Map<string, Listener[]>();

  constructor(url: string) {
    this.url = url;
    StubEventSource.instances.push(this);
  }

  addEventListener(type: string, listener: Listener) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  close() {
    this.closed = true;
  }

  emitOpen() {
    this.onopen?.(new Event("open"));
  }

  emitError() {
    this.onerror?.(new Event("error"));
  }

  emit(type: string, data: string) {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(new MessageEvent(type, { data }));
    }
  }

  static latest(): StubEventSource {
    const latest = StubEventSource.instances.at(-1);
    if (latest === undefined) {
      throw new Error("no EventSource was opened");
    }
    return latest;
  }
}

vi.stubGlobal("EventSource", StubEventSource);

afterEach(() => {
  cleanup();
  StubEventSource.instances = [];
  vi.restoreAllMocks();
});
