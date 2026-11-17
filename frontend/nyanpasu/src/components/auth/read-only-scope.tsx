import { type PropsWithChildren } from 'react'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { useCapabilities, type PermissionName } from './capabilities'

/**
 * Keeps the controls of a page visible but inert for a caller that lacks one of
 * `permissions`, and says so, instead of letting every control fire a rejected
 * request. A caller holding every permission sees `children` unchanged.
 *
 * The fieldset only disables form controls, so it generates no box
 * (`contents`) and the slider, which is not one, is disabled by its role.
 */
export function ReadOnlyScope({
  permissions,
  banner = true,
  children,
}: PropsWithChildren<{
  permissions: readonly PermissionName[]
  /** Leave the explanation to the one sibling scope that shows it. */
  banner?: boolean
}>) {
  const { hasPermissions } = useCapabilities()

  if (hasPermissions(permissions)) {
    return <>{children}</>
  }

  return (
    <>
      {banner && (
        <p
          role="status"
          data-slot="read-only-banner"
          className={cn(
            'bg-secondary-container text-on-secondary-container',
            'mx-4 mb-4 rounded-xl px-4 py-3 text-sm',
          )}
        >
          {m.auth_read_only_banner()}
        </p>
      )}

      <fieldset
        disabled
        data-slot="read-only-scope"
        className="contents [&_[role=slider]]:pointer-events-none"
      >
        {children}
      </fieldset>
    </>
  )
}
