import { cva, type VariantProps } from 'class-variance-authority'
import {
  createContext,
  useContext,
  useId,
  type ComponentProps,
  type ReactNode,
} from 'react'
import { cn } from '@nyanpasu/utils'
import { Button } from './button'
import { MaterialShape } from './material-shape'
import TextMarquee from './text-marquee'

const navigationListVariants = cva('flex flex-col gap-4 p-3')

const navigationListGroupVariants = cva('flex flex-col')

const navigationListGroupItemsVariants = cva([
  'flex flex-col gap-0.5',
  // Only the ends of a group get the large radius, like `SettingsGroup`.
  '[&>*:first-child:not(:only-child)]:rounded-b-sm',
  '[&>*:last-child:not(:only-child)]:rounded-t-sm',
  '[&>*:not(:first-child):not(:last-child)]:rounded-sm',
])

const navigationListItemVariants = cva([
  'group/navigation-list-item',
  'h-auto min-h-16 w-full justify-start rounded-3xl px-4 py-2.5',
  'flex items-center gap-3 text-left',
  'bg-surface text-on-surface dark:bg-surface dark:text-on-surface',
  // A state layer over the item's own fill instead of the button's hover fill.
  'data-[active=false]:hover:bg-surface',
  'hover:shadow-[inset_0_0_0_100px] hover:shadow-on-surface/8',
  'focus-visible:ring-primary focus-visible:ring-2 focus-visible:ring-inset',
  'transition-[border-radius,background-color,box-shadow] duration-300 ease-[cubic-bezier(0.2,0,0,1)]',
  'motion-reduce:transition-none',
  // The selected item rounds every corner, wherever it sits in its group.
  'data-[active=true]:bg-secondary-container data-[active=true]:hover:bg-secondary-container',
  'data-[active=true]:text-on-secondary-container data-[active=true]:rounded-3xl!',
  // The app owns drag and drop; it sets these attributes on the item.
  'data-[dragging=true]:z-10 data-[dragging=true]:rounded-3xl! data-[dragging=true]:shadow-lg',
  'data-[drop-target=true]:shadow-[inset_0_0_0_100px] data-[drop-target=true]:shadow-on-surface/8',
  'data-[drop-target=true]:ring-primary data-[drop-target=true]:ring-2 data-[drop-target=true]:ring-inset',
])

const navigationListItemIconVariants = cva(
  [
    'size-full transition-colors duration-200',
    '[&>svg]:size-5',
    'group-data-[active=true]/navigation-list-item:bg-primary',
    'group-data-[active=true]/navigation-list-item:text-on-primary',
  ],
  {
    variants: {
      color: {
        red: 'bg-red-container text-on-red-container',
        orange: 'bg-orange-container text-on-orange-container',
        yellow: 'bg-yellow-container text-on-yellow-container',
        green: 'bg-green-container text-on-green-container',
        cyan: 'bg-cyan-container text-on-cyan-container',
        blue: 'bg-blue-container text-on-blue-container',
        purple: 'bg-purple-container text-on-purple-container',
        pink: 'bg-pink-container text-on-pink-container',
        neutral: 'bg-surface-variant text-on-surface-variant',
      },
    },
  },
)

export type NavigationListItemColor = NonNullable<
  VariantProps<typeof navigationListItemIconVariants>['color']
>

const NavigationListItemContext = createContext({ active: false })

export function NavigationList({ className, ...props }: ComponentProps<'nav'>) {
  return (
    <nav
      className={cn(navigationListVariants(), className)}
      data-slot="navigation-list"
      {...props}
    />
  )
}

export function NavigationListGroup({
  label,
  action,
  className,
  children,
  ...props
}: Omit<ComponentProps<'section'>, 'children'> & {
  label: string
  /** Shown at the end of the heading row, such as an add button. */
  action?: ReactNode
  children: ReactNode
}) {
  const labelId = useId()

  return (
    <section
      className={cn(navigationListGroupVariants(), className)}
      data-slot="navigation-list-group"
      {...props}
    >
      <div className="flex items-center justify-between gap-2">
        <h2
          id={labelId}
          className="text-primary min-w-0 px-4 py-2 text-sm font-medium"
          data-slot="navigation-list-group-label"
        >
          {label}
        </h2>

        {action}
      </div>

      <div
        role="group"
        aria-labelledby={labelId}
        className={navigationListGroupItemsVariants()}
      >
        {children}
      </div>
    </section>
  )
}

/**
 * One row of a navigation list. The app computes `active` (this package has no
 * router) and may set `data-dragging` / `data-drop-target` from its drag and
 * drop logic; `ref` and every other prop reach the element.
 */
export function NavigationListItem({
  active,
  asChild,
  className,
  ...props
}: ComponentProps<'button'> & {
  active: boolean
  asChild?: boolean
}) {
  return (
    <NavigationListItemContext value={{ active }}>
      <Button
        className={cn(navigationListItemVariants(), className)}
        asChild={asChild}
        {...props}
        data-active={String(active)}
        data-slot="navigation-list-item"
      />
    </NavigationListItemContext>
  )
}

/**
 * The shape at the start of an item. A glyph child takes a tonal `color`. An
 * `image` child (an `<img>`) is the item's own picture: it is shown whole, with
 * no background, no clipping and no cropping, and it does not change when its
 * item is selected.
 */
export function NavigationListItemIcon({
  color,
  image,
  className,
  children,
  ...props
}: Omit<ComponentProps<'span'>, 'color'> &
  (
    | { color: NavigationListItemColor; image?: false }
    | { color?: undefined; image: true }
  )) {
  const { active } = useContext(NavigationListItemContext)

  return (
    <span
      className={cn(
        'flex size-8 shrink-0',
        image &&
          'items-center justify-center [&>img]:size-full [&>img]:object-contain',
        className,
      )}
      data-slot="navigation-list-item-icon"
      aria-hidden
      {...props}
    >
      {image ? (
        children
      ) : (
        <MaterialShape
          className={navigationListItemIconVariants({ color })}
          // A circle at rest; the selected item morphs it into a cookie.
          depth={active ? undefined : 0}
        >
          {children}
        </MaterialShape>
      )}
    </span>
  )
}

export function NavigationListItemText({
  headline,
  supporting,
  supportingStart,
  className,
  ...props
}: Omit<ComponentProps<'span'>, 'children'> & {
  headline: string
  supporting: string
  /** Fixed content before the supporting text, such as a chip or an icon. */
  supportingStart?: ReactNode
}) {
  const supportingText = (
    <TextMarquee className="text-on-surface-variant group-data-[active=true]/navigation-list-item:text-on-secondary-container/80 text-xs leading-4">
      {supporting}
    </TextMarquee>
  )

  return (
    <span
      className={cn('flex min-w-0 flex-1 flex-col gap-0.5', className)}
      {...props}
    >
      <span className="truncate text-sm leading-5 font-medium" title={headline}>
        {headline}
      </span>

      {supportingStart ? (
        <span className="flex min-w-0 items-center gap-1.5">
          {supportingStart}

          <span className="min-w-0 flex-1">{supportingText}</span>
        </span>
      ) : (
        supportingText
      )}
    </span>
  )
}

/**
 * Content after the text of an item, such as a status chip. It never shrinks,
 * so the text gives way instead.
 */
export function NavigationListItemTrailing({
  className,
  ...props
}: ComponentProps<'span'>) {
  return (
    <span
      className={cn('flex shrink-0 items-center', className)}
      data-slot="navigation-list-item-trailing"
      {...props}
    />
  )
}
