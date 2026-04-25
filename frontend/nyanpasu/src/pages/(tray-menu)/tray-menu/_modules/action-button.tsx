import ArrowRightRounded from '~icons/material-symbols/arrow-right-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import { ComponentProps, ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@nyanpasu/utils'
import { AsyncHandler, useTrayClickHandler } from './hooks'

export function ActionButton({
  checked,
  disableClose,
  onClick,
  className,
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
        'flex flex-row items-center justify-start gap-2 [&_svg]:size-4.5',
        'text-left font-semibold',
        'data-[checked=true]:bg-secondary-container',
        className,
      )}
      data-slot="tray-menu-action-button"
      data-checked={String(Boolean(checked))}
      variant="raised"
      onClick={handleClick}
      {...props}
    />
  )
}

// export function ActionButton({
//   checked,
//   prexixIcon,
//   suffixIcon,
//   children,
//   className,
//   ...props
// }: Omit<ComponentProps<typeof Button>, 'variant'> & {
//   checked?: boolean
//   prexixIcon?: ReactNode | boolean
//   suffixIcon?: ReactNode | boolean
// }) {
//   return (
//     <Button
//       className={cn(
//         'flex items-center justify-center gap-2 rounded-none px-4 text-left',
//         'shrink-0 transition-none',
//         className,
//       )}
//       variant={checked ? 'basic' : 'raised'}
//       data-slot="tray-menu-item"
//       data-checked={String(Boolean(checked))}
//       {...props}
//     >
//       <div
//         className="flex w-4 shrink-0 items-center justify-center"
//         data-slot="tray-menu-item-prefix-icon"
//       >
//         {typeof prexixIcon === 'boolean' ? <CheckRounded /> : prexixIcon}
//       </div>

//       <div className="flex-1" data-slot="tray-menu-item-content">
//         {children}
//       </div>

//       <div
//         className="flex w-4 shrink-0 items-center justify-center"
//         data-slot="tray-menu-item-suffix-icon"
//       >
//         {typeof suffixIcon === 'boolean' ? <ArrowRightRounded /> : suffixIcon}
//       </div>
//     </Button>
//   )
// }

export function ActionButtonSeparator() {
  return (
    <div className="bg-outline-variant h-px" data-slot="tray-menu-separator" />
  )
}
