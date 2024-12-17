import { Octokit } from 'octokit'
import { ProxyAgent, fetch as undiciFetch } from 'undici'
import { HTTP_PROXY } from './'

const BASE_OPTIONS = {
  owner: 'libnyanpasu',
  repo: 'clash-nyanpasu',
}

export const fetcher = (
  url: string,
  options: Parameters<typeof undiciFetch>[1] = {},
) => {
  return undiciFetch(url, {
    ...options,
    dispatcher: HTTP_PROXY ? new ProxyAgent(HTTP_PROXY) : undefined,
  })
}

export const octokit = new Octokit(applyProxy(BASE_OPTIONS))

export function applyProxy(opts: ConstructorParameters<typeof Octokit>[0]) {
  return {
    ...opts,
    request: {
      fetch: fetcher,
    },
    auth: process.env.GITHUB_TOKEN || process.env.GH_TOKEN || undefined,
  } satisfies ConstructorParameters<typeof Octokit>[0]
}
