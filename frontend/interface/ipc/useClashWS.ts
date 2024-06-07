import { useWebSocket } from "ahooks";
import { useClash } from "./useClash";
import { useMemo } from "react";

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
      };
    }
  }, [getClashInfo.data]);

  const connections = useWebSocket(url?.connections ?? "");

  const logs = useWebSocket(url?.logs ?? "");

  return {
    connections,
    logs,
  };
};
