import { useRef, useEffect } from "react";

export type WsMsgFn = (event: MessageEvent<any>) => void;

export interface WsOptions {
  errorCount?: number; // default is 5
  retryInterval?: number; // default is 2500
  onError?: () => void;
}

export const useWebsocket = (onMessage: WsMsgFn, options?: WsOptions) => {
  const wsRef = useRef<WebSocket | null>(null);
  const timerRef = useRef<number | null>(null);

  const disconnect = () => {
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }
  };

  const connect = (url: string) => {
    let errorCount = options?.errorCount ?? 5;
    const retryInterval = options?.retryInterval ?? 2500;

    if (!url) return;

    const connectHelper = () => {
      disconnect();

      const ws = new WebSocket(url);
      wsRef.current = ws;

      ws.addEventListener("message", onMessage);
      ws.addEventListener("error", () => {
        errorCount -= 1;

        if (errorCount >= 0) {
          timerRef.current = window.setTimeout(connectHelper, retryInterval);
        } else {
          disconnect();
          options?.onError?.();
        }
      });

      ws.addEventListener("close", () => {
        // WebSocket connection closed, attempt to reconnect
        timerRef.current = window.setTimeout(connectHelper, retryInterval);
      });
    };

    connectHelper();
  };

  useEffect(() => {
    // Cleanup on component unmount
    return () => {
      disconnect();
    };
  }, []);

  return { connect, disconnect };
};
