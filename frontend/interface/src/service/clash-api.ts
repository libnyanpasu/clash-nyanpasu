import { ofetch } from 'ofetch'
import { useMemo } from 'react'
import type { ProxyGroupItem, SubscriptionInfo } from '../ipc/bindings'
import { useClashInfo } from '../ipc/use-clash-info'

const prepareServer = (server?: string) => {
  if (server?.startsWith(':')) {
    return `127.0.0.1${server}`
  } else if (server && /^\d+$/.test(server)) {
    return `127.0.0.1:${server}`
  } else {
    return server
  }
}

export interface ClashConfig {
  port: number
  mode: string
  ipv6: boolean
  'socket-port': number
  'allow-lan': boolean
  'log-level': string
  'mixed-port': number
  'redir-port': number
  'socks-port': number
  'tproxy-port': number
  'external-controller': string
  secret: string
}

export type ClashVersion = {
  premium?: boolean
  meta?: boolean
  version: string
}

export type ClashDelayOptions = {
  url?: string
  timeout?: number
}

export type ClashProxyGroupItem = ProxyGroupItem

export type ClashProviderRule = {
  behavior: string
  format: string
  name: string
  ruleCount: number
  type: string
  updatedAt: string
  vehicleType: string
}

export type ClashProviderProxies = {
  name: string
  type: string
  proxies: ClashProxyGroupItem[]
  updatedAt?: string
  vehicleType: string
  subscriptionInfo?: SubscriptionInfo
  testUrl?: string
}

export type ClashRule = {
  type: string
  payload: string
  proxy: string
}

export const useClashAPI = () => {
  const { data } = useClashInfo()

  const request = useMemo(() => {
    return ofetch.create({
      baseURL: `http://${prepareServer(data?.server)}`,
      headers: data?.secret
        ? { Authorization: `Bearer ${data?.secret}` }
        : undefined,
    })
  }, [data])

  /**
   * Fetches Clash configurations from the server.
   */
  const configs = async () => {
    return await request<ClashConfig>('/configs')
  }

  /**
   * Update basic configuration; data must be sent in the format '{"mixed-port": 7890}',
   * modified as needed for the configuration items to be updated.
   */
  const patchConfigs = async (config: Partial<ClashConfig>) => {
    return await request<ClashConfig>('/configs', {
      method: 'PATCH',
      body: config,
    })
  }

  /**
   * Reload basic configuration; data must be sent, and the URL must include ?force=true to enforce execution.
   */
  const putConfigs = async (config: Partial<ClashConfig>, force?: boolean) => {
    const url = force ? '/configs?force=true' : '/configs'

    return await request<ClashConfig>(url, {
      method: 'PUT',
      body: config,
    })
  }

  const deleteConnections = async (id?: string) => {
    const url = id ? `/connections/${id}` : '/connections'

    return await request(url, {
      method: 'DELETE',
    })
  }

  const version = async () => {
    return await request<ClashVersion>('/version')
  }

  const proxiesDelay = async (name: string, options?: ClashDelayOptions) => {
    return await request<{ delay: number }>(
      `/proxies/${encodeURIComponent(name)}/delay`,
      {
        params: {
          timeout: options?.timeout || 10000,
          url: options?.url || 'http://www.gstatic.com/generate_204',
        },
      },
    )
  }

  const groupDelay = async (group: string, options?: ClashDelayOptions) => {
    return await request<Record<string, number>>(
      `/group/${encodeURIComponent(group)}/delay`,
      {
        params: {
          timeout: options?.timeout || 10000,
          url: options?.url || 'http://www.gstatic.com/generate_204',
        },
      },
    )
  }

  const proxies = async () => {
    return await request<{
      proxies: ClashProxyGroupItem[]
    }>('/proxies')
  }

  const putProxies = async ({
    group,
    proxy,
  }: {
    group: string
    proxy: string
  }) => {
    return await request(`/proxies/${encodeURIComponent(group)}`, {
      method: 'PUT',
      body: { name: proxy },
    })
  }

  const rules = async () => {
    return await request<{
      rules: ClashRule[]
    }>('/rules')
  }

  const providersRules = async () => {
    return await request<{ providers: Record<string, ClashProviderRule> }>(
      '/providers/rules',
    )
  }

  const putProvidersRules = async (name: string) => {
    return await request(`/providers/rules/${encodeURIComponent(name)}`, {
      method: 'PUT',
    })
  }

  const providersProxies = async (all?: string) => {
    const result = await request<{
      providers: Record<string, ClashProviderProxies>
    }>('/providers/proxies')

    if (all) {
      return result
    }

    return {
      providers: Object.fromEntries(
        Object.entries(result.providers).filter(([, value]) =>
          ['http', 'file'].includes(value.vehicleType.toLowerCase()),
        ),
      ),
    }
  }

  const putProvidersProxies = async (name: string) => {
    return await request(`/providers/proxies/${encodeURIComponent(name)}`, {
      method: 'PUT',
    })
  }

  return {
    configs,
    patchConfigs,
    putConfigs,
    deleteConnections,
    version,
    proxiesDelay,
    groupDelay,
    proxies,
    putProxies,
    rules,
    providersRules,
    putProvidersRules,
    providersProxies,
    putProvidersProxies,
  }
}
