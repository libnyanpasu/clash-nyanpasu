import { rpc } from '@/services/rpc'
import { probeWebviewCapabilities } from '@/utils/webview-capabilities'
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
  const page = await probeWebviewCapabilities()
  await rpc.reportIssue(page)
}
