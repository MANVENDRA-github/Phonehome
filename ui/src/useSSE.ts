// EventSource client for /api/stream. Pulses go to callback subscribers, NOT
// React state — a busy replay emits many per second and must not re-render the
// tree. Auto-reconnects with backoff; `status` lets callers keep the polling
// fallback alive while the stream is down.

import { useCallback, useEffect, useRef, useState } from "react";
import type { Pulse } from "./api";

export type SSEStatus = "connecting" | "open" | "error";

export function useSSE() {
  const subscribers = useRef(new Set<(p: Pulse) => void>());
  const [status, setStatus] = useState<SSEStatus>("connecting");

  useEffect(() => {
    let es: EventSource | null = null;
    let stopped = false;
    let retryMs = 1000;
    let timer = 0;

    const connect = () => {
      es = new EventSource("/api/stream");
      es.addEventListener("open", () => {
        setStatus("open");
        retryMs = 1000;
      });
      es.addEventListener("pulse", (e) => {
        try {
          const pulse = JSON.parse((e as MessageEvent).data) as Pulse;
          subscribers.current.forEach((cb) => cb(pulse));
        } catch {
          // malformed frame: ignore, pulses are hints
        }
      });
      es.addEventListener("error", () => {
        setStatus("error");
        es?.close();
        if (!stopped) {
          retryMs = Math.min(retryMs * 2, 15000);
          timer = window.setTimeout(connect, retryMs);
        }
      });
    };
    connect();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
      es?.close();
    };
  }, []);

  const subscribe = useCallback((cb: (p: Pulse) => void) => {
    subscribers.current.add(cb);
    return () => {
      subscribers.current.delete(cb);
    };
  }, []);

  return { status, subscribe };
}
