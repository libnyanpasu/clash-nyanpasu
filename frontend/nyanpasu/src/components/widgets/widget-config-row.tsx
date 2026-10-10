import { PropsWithChildren, ReactNode } from 'react'
import { LauncherMenuGroup } from '@nyanpasu/ui/launcher-menu'
import { cn } from '@nyanpasu/utils'

/** A launcher-menu group whose direct children are padded as config rows. */
export function ConfigGroup({ children }: PropsWithChildren) {
  return (
    <LauncherMenuGroup
      className="*:px-4 *:py-3"
      data-slot="widget-config-group"
    >
      {children}
    </LauncherMenuGroup>
  )
}

/** One-line row: the label on the left and its control on the right. */
export function ConfigRow({
  label,
  htmlFor,
  className,
  children,
}: PropsWithChildren<{
  label: ReactNode
  htmlFor?: string
  className?: string
}>) {
  const Label = htmlFor ? 'label' : 'span'

  return (
    <div
      className={cn('flex items-center justify-between gap-3', className)}
      data-slot="widget-config-row"
    >
      <Label htmlFor={htmlFor} className="min-w-0 text-sm">
        {label}
      </Label>

      {children}
    </div>
  )
}
