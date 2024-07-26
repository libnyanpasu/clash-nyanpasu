import { useWebSocket } from "ahooks";
import { useMemo } from "react";
import { useClash } from "./useClash";

export const useClashWS = () => {
  const { getClashInfo } = useClash();

  const getBaseUrl = () => {
    return `ws://${getClashInfo.data?.server}`;
  };

  const getTokenUrl = () => {
    return `token=${encodeURIComponent(getClashInfo.data?.secret || "")}`;
  };

  const resolveUrl = (path: string) => {
    return `${getBaseUrl()}/${path}?${getTokenUrl()}`;
  };

  const url = useMemo(() => {
    if (getClashInfo.data) {
      return {
        connections: resolveUrl("connections"),
        logs: resolveUrl("logs"),
        traffic: resolveUrl("traffic"),
        memory: resolveUrl("memory"),
      };
    }
  }, [getClashInfo.data]);

  const connections = useWebSocket(url?.connections ?? "");

  const logs = useWebSocket(url?.logs ?? "");

  const traffic = useWebSocket(url?.traffic ?? "");

  const memory = useWebSocket(url?.memory ?? "");

  return {
    connections,
    logs,
    traffic,
    memory,
  };
};
