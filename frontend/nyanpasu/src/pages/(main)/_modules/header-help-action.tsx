import BugReportRounded from '~icons/material-symbols/bug-report-rounded'
import DescriptionRounded from '~icons/material-symbols/description-rounded'
import HelpRounded from '~icons/material-symbols/help-rounded'
import InfoRounded from '~icons/material-symbols/info-rounded'
import {
  MenubarContent,
  MenubarGroup,
  MenubarMenu,
  MenubarTrigger,
} from '@nyanpasu/ui/menubar'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { useLockFn } from '@nyanpasu/hooks'
import { createBrowserAdapter } from '@nyanpasu/platform'
import { useMatchesPlatform } from '@nyanpasu/platform/react'
import { Link } from '@tanstack/react-router'
import { useHeaderMenuPrimitives } from './header-menu-primitives'
import { openIssueReport, openWiki } from './help-links'

const WikiItem = () => {
  const { mode, Item } = useHeaderMenuPrimitives()
  const handleClick = useLockFn(openWiki)

  return (
    <Item onSelect={handleClick}>
      {mode === 'launcher' && (
        <HelpRounded className="size-5 shrink-0" aria-hidden="true" />
      )}
      <span>{m.header_help_action_wiki()}</span>
    </Item>
  )
}

const IssuesItem = () => {
  const { mode, Item } = useHeaderMenuPrimitives()
  const handleClick = useLockFn(openIssueReport)

  return (
    <Item onSelect={handleClick}>
      {mode === 'launcher' && (
        <BugReportRounded className="size-5 shrink-0" aria-hidden="true" />
      )}
      <span>{m.header_help_action_issues()}</span>
    </Item>
  )
}

const CollectLogItem = () => {
  const { mode, Item } = useHeaderMenuPrimitives()
  const isDesktop = useMatchesPlatform('desktop')
  const handleClick = useLockFn(async () => {
    if (!isDesktop) {
      createBrowserAdapter()?.download('/bridge/logs/archive')
      return
    }

    await rpc.collectLogs()
  })

  return (
    <Item onSelect={handleClick}>
      {mode === 'launcher' && (
        <DescriptionRounded className="size-5 shrink-0" aria-hidden="true" />
      )}
      <span>{m.header_help_action_collect_logs()}</span>
    </Item>
  )
}

export function HeaderHelpItems() {
  const { mode, Item } = useHeaderMenuPrimitives()

  return (
    <>
      <WikiItem />

      <IssuesItem />

      <CollectLogItem />

      <Item asChild>
        <Link to="/main/settings/about">
          {mode === 'launcher' && (
            <InfoRounded className="size-5 shrink-0" aria-hidden="true" />
          )}
          <span>{m.header_help_action_about()}</span>
        </Link>
      </Item>
    </>
  )
}

export default function HeaderHelpAction() {
  return (
    <MenubarMenu value="help">
      <MenubarTrigger className="min-h-8 py-1">
        {m.header_help_action_title()}
      </MenubarTrigger>

      <MenubarContent>
        <MenubarGroup>
          <HeaderHelpItems />
        </MenubarGroup>
      </MenubarContent>
    </MenubarMenu>
  )
}
