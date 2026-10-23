import CircleRounded from '~icons/material-symbols/circle-rounded'
import ErrorRounded from '~icons/material-symbols/error-rounded'
import InputRounded from '~icons/material-symbols/input-rounded'
import OutputRounded from '~icons/material-symbols/output-rounded'
import RadioButtonUncheckedRounded from '~icons/material-symbols/radio-button-unchecked-rounded'
import StarRounded from '~icons/material-symbols/star-rounded'
import WarningRounded from '~icons/material-symbols/warning-rounded'
import type { ComponentType, SVGProps } from 'react'
import { cn } from '@nyanpasu/utils'
import { statusLabel } from './inspect-labels'
import type { StepStatus } from './inspect-steps'

const STATUS_ICON: Record<
  StepStatus,
  { icon: ComponentType<SVGProps<SVGSVGElement>>; className: string }
> = {
  start: { icon: InputRounded, className: 'text-ok' },
  changed: { icon: CircleRounded, className: 'text-ok' },
  unchanged: {
    icon: RadioButtonUncheckedRounded,
    className: 'text-on-surface-variant opacity-60',
  },
  warn: { icon: WarningRounded, className: 'text-warn' },
  error: { icon: ErrorRounded, className: 'text-error' },
  final: { icon: StarRounded, className: 'text-ok' },
  core: { icon: OutputRounded, className: 'text-ok' },
}

/** The icon that says how a step went; it is the only mark of a status. */
export default function StatusIcon({
  status,
  className,
}: {
  status: StepStatus
  className?: string
}) {
  const { icon: Icon, className: iconClassName } = STATUS_ICON[status]

  return (
    <Icon
      role="img"
      aria-label={statusLabel(status)}
      className={cn('size-5 shrink-0', iconClassName, className)}
    />
  )
}
