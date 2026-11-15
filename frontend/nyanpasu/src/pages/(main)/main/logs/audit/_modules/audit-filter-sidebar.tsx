import CancelRounded from '~icons/material-symbols/cancel-rounded'
import CheckCircleRounded from '~icons/material-symbols/check-circle-rounded'
import ErrorRounded from '~icons/material-symbols/error-rounded'
import FilterListRounded from '~icons/material-symbols/filter-list-rounded'
import HubRounded from '~icons/material-symbols/hub-rounded'
import LanguageRounded from '~icons/material-symbols/language-rounded'
import TerminalRounded from '~icons/material-symbols/terminal-rounded'
import { memo, type ReactNode } from 'react'
import {
  NavigationSidebar,
  NavigationSidebarContent,
  NavigationSidebarItem,
  NavigationSidebarToggle,
} from '@nyanpasu/ui/navigation-sidebar'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { Separator } from '@nyanpasu/ui/separator'
import { SidebarProvider } from '@nyanpasu/ui/slider-sidebar'
import { m } from '@/paraglide/messages'
import { Link } from '@tanstack/react-router'

/** The stored transport spellings the page filters by. */
export const AUDIT_TRANSPORTS = ['http', 'local'] as const

/** The call results the page filters by. */
export const AUDIT_OUTCOMES = ['success', 'failure', 'rejected'] as const

export type AuditTransport = (typeof AUDIT_TRANSPORTS)[number]

export type AuditOutcome = (typeof AUDIT_OUTCOMES)[number]

export type AuditSearch = {
  transport?: AuditTransport
  outcome?: AuditOutcome
}

const TransportIcon = {
  http: LanguageRounded,
  local: TerminalRounded,
}

const OutcomeIcon = {
  success: CheckCircleRounded,
  failure: ErrorRounded,
  rejected: CancelRounded,
}

export const transportLabel = (transport: AuditTransport) =>
  ({
    http: m.logs_audit_transport_http(),
    local: m.logs_audit_transport_local(),
  })[transport]

const outcomeLabel = (outcome: AuditOutcome) =>
  ({
    success: m.logs_audit_outcome_success(),
    failure: m.logs_audit_outcome_failure(),
    rejected: m.logs_audit_outcome_rejected(),
  })[outcome]

function AuditFilterItem({
  active,
  icon,
  search,
  children,
}: {
  active: boolean
  icon: ReactNode
  search: AuditSearch
  children: string
}) {
  return (
    <NavigationSidebarItem
      active={active}
      icon={icon}
      renderLink={(content) => (
        <Link
          from="/main/logs/audit/"
          to="."
          search={(previous) => ({ ...previous, ...search })}
        >
          {content}
        </Link>
      )}
    >
      {children}
    </NavigationSidebarItem>
  )
}

// The sidebar holds its own open state, so the table beside it can re-render
// without re-rendering the filter items.
export const AuditFilterSidebar = memo(function AuditFilterSidebar({
  transport,
  outcome,
}: AuditSearch) {
  return (
    <SidebarProvider defaultOpen={false}>
      <NavigationSidebar aria-label={m.logs_audit_filter_sidebar()}>
        <ScrollArea className="min-h-0 w-full flex-1 [&>div>div]:block!">
          <NavigationSidebarContent
            className="pb-16"
            data-slot="logs-audit-filters"
          >
            <AuditFilterItem
              active={!transport}
              icon={<HubRounded />}
              search={{ transport: undefined }}
            >
              {m.logs_audit_all_transports()}
            </AuditFilterItem>

            {AUDIT_TRANSPORTS.map((item) => {
              const Icon = TransportIcon[item]
              return (
                <AuditFilterItem
                  key={item}
                  active={transport === item}
                  icon={<Icon />}
                  search={{ transport: item }}
                >
                  {transportLabel(item)}
                </AuditFilterItem>
              )
            })}

            <Separator />

            <AuditFilterItem
              active={!outcome}
              icon={<FilterListRounded />}
              search={{ outcome: undefined }}
            >
              {m.logs_audit_all_outcomes()}
            </AuditFilterItem>

            {AUDIT_OUTCOMES.map((item) => {
              const Icon = OutcomeIcon[item]
              return (
                <AuditFilterItem
                  key={item}
                  active={outcome === item}
                  icon={<Icon />}
                  search={{ outcome: item }}
                >
                  {outcomeLabel(item)}
                </AuditFilterItem>
              )
            })}
          </NavigationSidebarContent>
        </ScrollArea>

        <NavigationSidebarToggle aria-label={m.logs_audit_filter_sidebar()}>
          {m.navigation_sidebar_collapse()}
        </NavigationSidebarToggle>
      </NavigationSidebar>
    </SidebarProvider>
  )
})
