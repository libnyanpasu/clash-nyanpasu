import { ComponentProps, CSSProperties } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@nyanpasu/utils'
import { AsyncHandler, useTrayClickHandler } from './hooks'

export function ActionButton({
  checked,
  disableClose,
  onClick,
  className,
  style,
  ...props
}: Omit<ComponentProps<typeof Button>, 'variant' | 'onClick'> & {
  checked?: boolean | null
  disableClose?: boolean
  onClick?: AsyncHandler
}) {
  const handleClick = useTrayClickHandler(onClick, disableClose)

  return (
    <Button
      className={cn(
        'bg-surface-variant/30 rounded-2xl px-3 py-2',
        'flex w-full min-w-0 flex-row items-center justify-start gap-2 overflow-hidden',
        'text-left font-semibold',
        'data-[checked=true]:bg-secondary-container',
        className,
      )}
      data-slot="tray-menu-action-button"
      data-checked={String(Boolean(checked))}
      style={
        {
          '--fallback-bg': checked
            ? 'var(--color-secondary-container)'
            : 'var(--color-surface-variant)',
          ...style,
        } as CSSProperties
      }
      variant="raised"
      onClick={handleClick}
      {...props}
    />
  )
}

export function ActionButtonSeparator() {
  return (
    <div className="bg-outline-variant h-px" data-slot="tray-menu-separator" />
  )
}
