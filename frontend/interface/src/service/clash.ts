import { ofetch } from 'ofetch'
import { getClashInfo } from './tauri'
import { ProviderItem } from './types'

// eslint-disable-next-line @typescript-eslint/no-namespace
export namespace Clash {
  export interface Config {
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

  export interface Version {
    premium: boolean
    meta?: boolean
    version: string
  }

  export interface Rule {
    type: string
    payload: string
    proxy: string
  }

  export interface Proxy<T = string> {
    name: string
    type: string
    udp: boolean
    xudp?: boolean
    history: {
      time: string
      delay: number
    }[]
    all?: T[]
    now?: string
    provider?: string
    alive?: boolean
    tfo?: boolean
    icon?: string
    hidden?: boolean
  }

  export interface DelayOptions {
    url?: string
    timeout?: number
  }
}

const prepareServer = (server: string) => {
  if (server.startsWith(':')) {
    return `127.0.0.1${server}`
  } else if (/^\d+$/.test(server)) {
    return `127.0.0.1:${server}`
  } else {
    return server
  }
}

export const clash = () => {
  const buildRequest = async () => {
    const info = await getClashInfo()

    return ofetch.create({
      baseURL: `http://${prepareServer(info?.server as string)}`,
      headers: info?.secret
        ? { Authorization: `Bearer ${info?.secret}` }
        : undefined,
    })
  }

  const getConfigs = async () => {
    return (await buildRequest())<Clash.Config>('/configs')
  }

  const setConfigs = async (config: Partial<Clash.Config>) => {
    return (await buildRequest())<Clash.Config>('/configs', {
      method: 'PATCH',
      body: config,
    })
  }

  const getVersion = async () => {
    return (await buildRequest())<Clash.Version>('/version')
  }

  const getRules = async () => {
    return (await buildRequest())<{
      rules: Clash.Rule[]
    }>('/rules')
  }

  const getProxiesDelay = async (
    name: string,
    options?: Clash.DelayOptions,
  ) => {
    return (await buildRequest())<{ delay: number }>(
      `/proxies/${encodeURIComponent(name)}/delay`,
      {
        params: {
          timeout: options?.timeout || 10000,
          url: options?.url || 'http://www.gstatic.com/generate_204',
        },
      },
    )
  }

  const getGroupDelay = async (group: string, options?: Clash.DelayOptions) => {
    return (await buildRequest())<{ [key: string]: number }>(
      `/group/${encodeURIComponent(group)}/delay`,
      {
        params: {
          timeout: options?.timeout || 10000,
          url: options?.url || 'http://www.gstatic.com/generate_204',
        },
      },
    )
  }

  const getProxies = async () => {
    return (await buildRequest())<{
      proxies: Clash.Proxy[]
    }>('/proxies')
  }

  const setProxies = async ({
    group,
    proxy,
  }: {
    group: string
    proxy: string
  }) => {
    return (await buildRequest())(`/proxies/${encodeURIComponent(group)}`, {
      method: 'PUT',
      body: { name: proxy },
    })
  }

  const deleteConnections = async (id?: string) => {
    return (await buildRequest())(id ? `/connections/${id}` : '/connections', {
      method: 'DELETE',
    })
  }

  const getRulesProviders = async () => {
    return (
      await (
        await buildRequest()
      )('/providers/rules', {
        method: 'GET',
      })
    )?.providers
  }

  const updateRulesProviders = async (name: string) => {
    return (await buildRequest())(`/providers/rules/${name}`, {
      method: 'PUT',
    })
  }

  const getProxiesProviders = async () => {
    const result: { [key: string]: ProviderItem } = (
      await (
        await buildRequest()
      )('/providers/proxies')
    )?.providers

    const types = ['http', 'file']

    return Object.fromEntries(
      Object.entries(result).filter(([, value]) =>
        types.includes(value.vehicleType.toLowerCase()),
      ),
    )
  }

  const getAllProxiesProviders = async () => {
    return (await (await buildRequest())('/providers/proxies'))?.providers
  }

  const updateProxiesProviders = async (name: string) => {
    return (await buildRequest())(
      `/providers/proxies/${encodeURIComponent(name)}`,
      {
        method: 'PUT',
      },
    )
  }

  return {
    getConfigs,
    setConfigs,
    getVersion,
    getRules,
    getProxiesDelay,
    getGroupDelay,
    getProxies,
    setProxies,
    deleteConnections,
    getRulesProviders,
    updateRulesProviders,
    getProxiesProviders,
    getAllProxiesProviders,
    updateProxiesProviders,
  }
}
