import {
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarTrigger,
} from '@nyanpasu/ui/menubar'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { useLockFn } from '@nyanpasu/hooks'
import { createBrowserAdapter } from '@nyanpasu/platform'
import { useMatchesPlatform } from '@nyanpasu/platform/react'
import { Link } from '@tanstack/react-router'
import { openIssueReport, openWiki } from './help-links'

const WikiItem = () => {
  const handleClick = useLockFn(openWiki)

  return (
    <MenubarItem onSelect={handleClick}>
      {m.header_help_action_wiki()}
    </MenubarItem>
  )
}

const IssuesItem = () => {
  const handleClick = useLockFn(openIssueReport)

  return (
    <MenubarItem onSelect={handleClick}>
      {m.header_help_action_issues()}
    </MenubarItem>
  )
}

const CollectLogItem = () => {
  const isDesktop = useMatchesPlatform('desktop')
  const handleClick = useLockFn(async () => {
    if (!isDesktop) {
      createBrowserAdapter()?.download('/bridge/logs/archive')
      return
    }

    await rpc.collectLogs()
  })

  return (
    <MenubarItem onSelect={handleClick}>
      {m.header_help_action_collect_logs()}
    </MenubarItem>
  )
}

export default function HeaderHelpAction() {
  return (
    <MenubarMenu value="help">
      <MenubarTrigger className="min-h-8 py-1">
        {m.header_help_action_title()}
      </MenubarTrigger>

      <MenubarContent>
        <WikiItem />

        <IssuesItem />

        <CollectLogItem />

        <MenubarItem asChild>
          <Link to="/main/settings/about">{m.header_help_action_about()}</Link>
        </MenubarItem>
      </MenubarContent>
    </MenubarMenu>
  )
}
