import { ofetch } from "ofetch";
import { getClashInfo } from "./tauri";

export namespace Clash {
  export interface Config {
    port: number;
    mode: string;
    ipv6: boolean;
    "socket-port": number;
    "allow-lan": boolean;
    "log-level": string;
    "mixed-port": number;
    "redir-port": number;
    "socks-port": number;
    "tproxy-port": number;
    "external-controller": string;
    secret: string;
  }

  export interface Version {
    premium: boolean;
    meta?: boolean;
    version: string;
  }

  export interface Rule {
    type: string;
    payload: string;
    proxy: string;
  }

  export interface Proxy {
    name: string;
    type: string;
    udp: boolean;
    history: {
      time: string;
      delay: number;
    }[];
    all?: string[];
    now?: string;
    provider?: string;
  }
}

const prepareServer = (server: string) => {
  if (server.startsWith(":")) {
    return `127.0.0.1${server}`;
  } else if (/^\d+$/.test(server)) {
    return `127.0.0.1:${server}`;
  } else {
    return server;
  }
};

export const clash = () => {
  const buildRequest = async () => {
    const info = await getClashInfo();

    return ofetch.create({
      baseURL: `http://${prepareServer(info?.server as string)}`,
      headers: info?.secret
        ? { Authorization: `Bearer ${info?.secret}` }
        : undefined,
    });
  };

  const getConfigs = async () => {
    return (await buildRequest())<Clash.Config>("/configs");
  };

  const setConfigs = async (config: Partial<Clash.Config>) => {
    return (await buildRequest())<Clash.Config>("/configs", {
      method: "PATCH",
      body: config,
    });
  };

  const getVersion = async () => {
    return (await buildRequest())<Clash.Version>("/version");
  };

  const getRules = async () => {
    interface PrivateRule {
      rules: Clash.Rule[];
    }

    return (await buildRequest())<PrivateRule>("/rules");
  };

  const getProxiesDelay = async (
    name: string,
    options?: {
      url?: string;
      timeout?: number;
    },
  ) => {
    return (await buildRequest())<{ delay: number }>(
      `/proxies/${encodeURIComponent(name)}/delay`,
      {
        params: {
          timeout: options?.timeout || 10000,
          url: options?.url || "http://www.gstatic.com/generate_204",
        },
      },
    );
  };

  const getProxies = async () => {
    interface PrivateProxy {
      proxies: Clash.Proxy[];
    }

    return (await buildRequest())<PrivateProxy>("/proxies");
  };

  const setProxies = async ({
    group,
    proxy,
  }: {
    group: string;
    proxy: string;
  }) => {
    return (await buildRequest())(`/proxies/${encodeURIComponent(group)}`, {
      method: "PUT",
      body: { name: proxy },
    });
  };

  const deleteConnections = async () => {
    return (await buildRequest())(`/connections`, {
      method: "DELETE",
    });
  };

  return {
    getConfigs,
    setConfigs,
    getVersion,
    getRules,
    getProxiesDelay,
    getProxies,
    setProxies,
    deleteConnections,
  };
};
