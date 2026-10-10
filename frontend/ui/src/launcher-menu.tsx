import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

/** Row layout shared by launcher-style menu rows, including Radix menu items. */
export const launcherMenuRowClassName = cn(
  'flex min-h-12 items-center gap-3 px-4 text-sm outline-hidden',
  'text-on-surface',
)

/** Compact value pill shown at the end of a row, such as a select trigger. */
export const launcherMenuChipClassName = cn(
  'bg-secondary-container text-on-secondary-container h-8 w-auto max-w-44 flex-none',
  'rounded-full px-3 py-0 text-sm',
)

/** Stack of launcher-style groups separated by a small gap. */
export function LauncherMenu({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="launcher-menu"
      className={cn('flex flex-col gap-1', className)}
      {...props}
    />
  )
}

/**
 * Rows share one large outer radius and are separated by a hairline gap; the
 * children are styled by position so Radix items and plain rows look alike.
 */
export function LauncherMenuGroup({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      role="group"
      data-slot="launcher-menu-group"
      className={cn(
        'flex flex-col gap-0.5',
        '*:bg-surface *:rounded-md *:border-0',
        '*:first:rounded-t-3xl *:last:rounded-b-3xl',
        className,
      )}
      {...props}
    />
  )
}

export function LauncherMenuRow({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      data-slot="launcher-menu-row"
      className={cn(launcherMenuRowClassName, className)}
      {...props}
    />
  )
}

/** Small round button at the end of a row, such as the info button in Android. */
export function LauncherMenuIconButton({
  className,
  type = 'button',
  ...props
}: ComponentProps<'button'>) {
  return (
    <button
      type={type}
      data-slot="launcher-menu-icon-button"
      className={cn(
        'text-on-surface-variant hover:bg-on-surface/8 focus-visible:ring-primary',
        'grid size-8 shrink-0 cursor-pointer place-items-center rounded-full outline-none focus-visible:ring-2',
        'disabled:pointer-events-none disabled:opacity-50',
        className,
      )}
      {...props}
    />
  )
}

/** Value pill that acts as a button, for two-way choices that flip on press. */
export function LauncherMenuChipButton({
  className,
  type = 'button',
  ...props
}: ComponentProps<'button'>) {
  return (
    <button
      type={type}
      data-slot="launcher-menu-chip-button"
      className={cn(
        launcherMenuChipClassName,
        'focus-visible:ring-primary inline-flex cursor-pointer items-center gap-1 outline-none hover:brightness-95 focus-visible:ring-2',
        'disabled:pointer-events-none disabled:opacity-50',
        className,
      )}
      {...props}
    />
  )
}
