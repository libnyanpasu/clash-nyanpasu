import { rpc } from '@/services/rpc'
import { formatEnvInfos } from '@/utils'
import { isTauri } from '@nyanpasu/platform'

const REPOSITORY_URL = 'https://github.com/libnyanpasu/clash-nyanpasu'

async function openHelpLink(url: string) {
  if (isTauri()) return rpc.openThat(url)
  window.location.assign(url)
}

export function openWiki() {
  return openHelpLink('https://nyanpasu.org')
}

export function openRepository() {
  return openHelpLink(REPOSITORY_URL)
}

/** Opens a new bug report prefilled with this environment's details. */
export async function openIssueReport() {
  const envs = await rpc.collectEnvs()

  if (envs.status !== 'ok') {
    return
  }

  const formattedEnv = encodeURIComponent(
    formatEnvInfos(envs.data)
      .split('\n')
      .map((v) => `> ${v}`)
      .join('\n'),
  )

  const params = new URLSearchParams({
    assignees: '',
    labels: 'T%3A+Bug%2CS%3A+Untriaged',
    projects: '',
    template: 'bug_report.yaml',
  })

  return openHelpLink(
    `${REPOSITORY_URL}/issues/new?` +
      params.toString() +
      // envs can't be serialized
      '&env_infos=' +
      formattedEnv,
  )
}
