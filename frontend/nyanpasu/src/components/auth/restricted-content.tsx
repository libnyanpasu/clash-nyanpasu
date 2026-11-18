import LockRounded from '~icons/material-symbols/lock-rounded'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'

/**
 * Stands in for a view whose data the caller may not read. The view is never
 * mounted, so its queries are not sent and cannot come back rejected.
 */
export function RestrictedContent() {
  return (
    <EmptyState
      className="flex-1"
      tone="normal"
      icon={LockRounded}
      message={m.auth_restricted_title()}
      description={m.auth_restricted_description()}
    />
  )
}
