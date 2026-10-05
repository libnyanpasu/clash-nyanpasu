import { PropsWithChildren } from 'react'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatEnvInfos } from '@/utils'
import { useLockFn } from '@nyanpasu/hooks'
import { isBrowser, isTauri } from '@nyanpasu/platform'
import { unwrapResult } from '@nyanpasu/rpc'
import { Link } from '@tanstack/react-router'

async function openHelpLink(url: string) {
  if (isTauri()) return rpc.openThat(url)
  window.location.assign(url)
}

const WikiItem = () => {
  const handleClick = useLockFn(async () => {
    await openHelpLink('https://nyanpasu.org')
  })

  return (
    <DropdownMenuItem onClick={handleClick}>
      {m.header_help_action_wiki()}
    </DropdownMenuItem>
  )
}

const IssuesItem = () => {
  const handleClick = useLockFn(async () => {
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
      'https://github.com/libnyanpasu/clash-nyanpasu/issues/new?' +
        params.toString() +
        // envs can't be serialized
        '&env_infos=' +
        formattedEnv,
    )
  })

  return (
    <DropdownMenuItem onClick={handleClick}>
      {m.header_help_action_issues()}
    </DropdownMenuItem>
  )
}

const CollectLogItem = () => {
  const handleClick = useLockFn(async () => {
    if (isBrowser()) {
      const archive = unwrapResult(await rpc.getLogsArchive())
      const url = URL.createObjectURL(
        new Blob([new Uint8Array(archive.bytes)], { type: 'application/zip' }),
      )
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = archive.fileName
      anchor.click()
      URL.revokeObjectURL(url)
      return
    }

    await rpc.collectLogs()
  })

  return (
    <DropdownMenuItem onClick={handleClick}>
      {m.header_help_action_collect_logs()}
    </DropdownMenuItem>
  )
}

export default function HeaderHelpAction({ children }: PropsWithChildren) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>

      <DropdownMenuContent>
        <WikiItem />

        <IssuesItem />

        <CollectLogItem />

        <DropdownMenuItem asChild>
          <Link to="/main/settings/about">{m.header_help_action_about()}</Link>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
