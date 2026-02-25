import { PropsWithChildren } from 'react'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatEnvInfos } from '@/utils'
import { commands } from '@nyanpasu/interface'
import { Link } from '@tanstack/react-router'

const WikiItem = () => {
  const handleClick = useLockFn(async () => {
    await commands.openThat('https://nyanpasu.elaina.moe')
  })

  return (
    <DropdownMenuItem onClick={handleClick}>
      {m.header_help_action_wiki()}
    </DropdownMenuItem>
  )
}

const IssuesItem = () => {
  const handleClick = useLockFn(async () => {
    const envs = await commands.collectEnvs()

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

    return commands.openThat(
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
    await commands.collectLogs()
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
