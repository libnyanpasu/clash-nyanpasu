import { PropsWithChildren } from 'react'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { useLockFn } from '@nyanpasu/hooks'
import { isBrowser } from '@nyanpasu/platform'
import { Link } from '@tanstack/react-router'
import { openIssueReport, openWiki } from './help-links'

const WikiItem = () => {
  const handleClick = useLockFn(openWiki)

  return (
    <DropdownMenuItem onClick={handleClick}>
      {m.header_help_action_wiki()}
    </DropdownMenuItem>
  )
}

const IssuesItem = () => {
  const handleClick = useLockFn(openIssueReport)

  return (
    <DropdownMenuItem onClick={handleClick}>
      {m.header_help_action_issues()}
    </DropdownMenuItem>
  )
}

const CollectLogItem = () => {
  const handleClick = useLockFn(async () => {
    if (isBrowser()) {
      const anchor = document.createElement('a')
      anchor.href = '/bridge/logs/archive'
      anchor.click()
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
