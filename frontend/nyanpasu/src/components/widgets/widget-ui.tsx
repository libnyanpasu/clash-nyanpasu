import type { ComponentProps, ComponentType, ReactNode } from 'react'
import { CardHeader } from '@nyanpasu/ui/card'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { cn } from '@nyanpasu/utils'

type WidgetHeaderProps = ComponentProps<typeof CardHeader>

export function WidgetHeader({ className, ...props }: WidgetHeaderProps) {
  return (
    <CardHeader
      className={cn('shrink-0 gap-3 px-4 pt-4 pb-0', className)}
      {...props}
    />
  )
}

type WidgetTitleProps = Omit<ComponentProps<'div'>, 'children'> & {
  icon: ComponentType<{ className?: string }>
  children: ReactNode
}

export function WidgetTitle({
  icon: Icon,
  className,
  children,
  ...props
}: WidgetTitleProps) {
  return (
    <div
      className={cn(
        'flex min-w-0 items-center gap-2 text-base font-bold',
        className,
      )}
      {...props}
    >
      <Icon className="size-5 shrink-0" aria-hidden="true" />
      <TextMarquee className="min-w-0 flex-1">{children}</TextMarquee>
    </div>
  )
}

type WidgetMetricProps = ComponentProps<'div'>

export function WidgetMetric({ className, ...props }: WidgetMetricProps) {
  return (
    <div
      className={cn('text-2xl font-bold text-nowrap text-shadow-md', className)}
      {...props}
    />
  )
}

type WidgetMetaProps = ComponentProps<'div'>

export function WidgetMeta({ className, ...props }: WidgetMetaProps) {
  return (
    <div
      className={cn(
        'text-shadow-background h-5 text-sm text-nowrap text-shadow-xs',
        className,
      )}
      {...props}
    />
  )
}
