import { cva, type VariantProps } from 'class-variance-authority'
import {
  useMemo,
  useRef,
  type ComponentProps,
  type ComponentType,
  type ReactNode,
} from 'react'
import { Card, type CardProps } from '@nyanpasu/ui/card'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { cn } from '@nyanpasu/utils'
import { useWidgetContext } from './widget-context'
import { WidgetSurfaceContext, type WidgetScalingMode } from './widget-surface'

const widgetRootVariants = cva([
  'relative isolate flex size-full min-h-0 flex-col overflow-hidden',
])

export type WidgetProps = Omit<ComponentProps<'div'>, 'ref'> & {
  scaling?: WidgetScalingMode
  variant?: CardProps['variant']
}

/**
 * The widget surface. It establishes the `widget` size container and exposes
 * the selected scaling curve through the `--w-*` tokens in widget-scaling.css.
 */
export function Widget({
  scaling = 'proportional',
  variant,
  className,
  children,
  ...props
}: WidgetProps) {
  const { type, mode } = useWidgetContext()
  const containerRef = useRef<HTMLDivElement>(null)
  const surface = useMemo(
    () => ({ containerRef, scaling }),
    [containerRef, scaling],
  )

  return (
    <WidgetSurfaceContext.Provider value={surface}>
      <Card asChild variant={variant}>
        <div
          ref={containerRef}
          data-slot="widget-root"
          data-widget-type={type}
          data-widget-mode={mode}
          data-scaling={scaling}
          className={cn(widgetRootVariants(), className)}
          {...props}
        >
          {children}
        </div>
      </Card>
    </WidgetSurfaceContext.Provider>
  )
}

export function WidgetHeader({
  className,
  ...props
}: ComponentProps<'header'>) {
  return (
    <header
      data-slot="widget-header"
      className={cn(
        'relative z-10 flex shrink-0 items-center justify-between',
        'gap-[var(--w-gap-sm)] px-[var(--w-pad-x)] pt-[var(--w-pad-y)] pb-0',
        className,
      )}
      {...props}
    />
  )
}

const widgetIconVariants = cva('shrink-0', {
  variants: {
    size: {
      sm: 'size-[var(--w-icon-sm)]',
      md: 'size-[var(--w-icon-md)]',
      lg: 'size-[var(--w-icon-lg)]',
    },
  },
  defaultVariants: {
    size: 'md',
  },
})

export type WidgetIconSize = NonNullable<
  VariantProps<typeof widgetIconVariants>['size']
>

export type WidgetIconProps = {
  icon?: ComponentType<{ className?: string }>
  size?: WidgetIconSize
  className?: string
}

/** Renders the definition icon unless the widget supplies a local override. */
export function WidgetIcon({ icon, size, className }: WidgetIconProps) {
  const { definition } = useWidgetContext()
  const Icon = icon ?? definition.icon

  return (
    <Icon
      data-slot="widget-icon"
      aria-hidden="true"
      className={cn(widgetIconVariants({ size }), className)}
    />
  )
}

export type WidgetTitleProps = Omit<ComponentProps<'div'>, 'children'> & {
  icon?: ComponentType<{ className?: string }>
  children?: ReactNode
}

/** The title falls back to the host's definition title and icon. */
export function WidgetTitle({
  icon,
  children,
  className,
  ...props
}: WidgetTitleProps) {
  const { definition } = useWidgetContext()
  const label = children ?? definition.title()

  return (
    <div
      data-slot="widget-title"
      className={cn(
        'flex min-w-0 flex-1 items-center',
        'gap-[var(--w-gap-sm)] text-[length:var(--w-text-title)] leading-tight font-bold',
        className,
      )}
      {...props}
    >
      <WidgetIcon icon={icon} />

      <TextMarquee className="min-w-0 flex-1">{label}</TextMarquee>
    </div>
  )
}

export function WidgetControls({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="widget-controls"
      className={cn(
        'pointer-events-auto relative z-20 flex shrink-0 items-center',
        'gap-[var(--w-gap-xs)]',
        className,
      )}
      {...props}
    />
  )
}

const widgetContentVariants = cva(
  [
    'relative z-10 flex min-h-0 flex-1 flex-col overflow-hidden',
    'gap-[var(--w-gap)]',
  ],
  {
    variants: {
      layout: {
        stack: 'justify-start',
        spread: 'justify-between',
        center: 'items-center justify-center text-center',
        flush: '',
      },
    },
    compoundVariants: [
      {
        layout: ['stack', 'spread', 'center'],
        className: 'px-[var(--w-pad-x)] py-[var(--w-pad-y)]',
      },
    ],
    defaultVariants: {
      layout: 'stack',
    },
  },
)

export type WidgetContentProps = ComponentProps<'div'> &
  VariantProps<typeof widgetContentVariants>

export function WidgetContent({
  layout,
  className,
  ...props
}: WidgetContentProps) {
  return (
    <div
      data-slot="widget-content"
      className={cn(widgetContentVariants({ layout }), className)}
      {...props}
    />
  )
}

export function WidgetMetric({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="widget-metric"
      className={cn(
        'text-[length:var(--w-text-metric)] leading-none font-bold',
        'text-nowrap tabular-nums text-shadow-md',
        className,
      )}
      {...props}
    />
  )
}

export function WidgetMeta({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="widget-meta"
      className={cn(
        'text-on-surface-variant text-nowrap tabular-nums',
        'text-[length:var(--w-text-meta)] leading-snug text-shadow-xs',
        className,
      )}
      {...props}
    />
  )
}

export function WidgetFooter({
  className,
  ...props
}: ComponentProps<'footer'>) {
  return (
    <footer
      data-slot="widget-footer"
      className={cn(
        'relative z-10 mt-auto flex shrink-0 items-center justify-between',
        'gap-[var(--w-gap-sm)] px-[var(--w-pad-x)] pt-0 pb-[var(--w-pad-y)]',
        className,
      )}
      {...props}
    />
  )
}
