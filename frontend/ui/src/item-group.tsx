import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import { cva, type VariantProps } from 'class-variance-authority'
import { Slot } from 'radix-ui'
import {
  cloneElement,
  isValidElement,
  useId,
  useRef,
  type ComponentProps,
  type MouseEvent,
  type MouseEventHandler,
  type PropsWithChildren,
  type ReactNode,
  type RefObject,
} from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import { Ripple, useRipple } from './ripple'
import { Switch } from './switch'

export const itemSectionVariants = cva(['flex min-w-0 flex-col'])

export const itemSectionHeaderVariants = cva([
  'flex items-center justify-between gap-2 px-4 pt-3 pb-2',
])

export const itemSectionTitleVariants = cva([
  'text-primary text-sm leading-5 font-medium',
])

export const itemSectionDescriptionVariants = cva([
  'text-on-surface-variant px-4 pb-2 text-sm leading-5',
])

export const itemGroupVariants = cva([
  'flex min-w-0 flex-col gap-0.5',
  '*:transition-[border-radius]',
  '[&>*:first-child:not(:only-child)]:rounded-b-extra-small',
  '[&>*:last-child:not(:only-child)]:rounded-t-extra-small',
  '[&>*:not(:first-child):not(:last-child)]:rounded-extra-small',
])

export const itemVariants = cva(
  [
    'relative flex min-h-16 w-full min-w-0 items-center gap-4 px-5 py-3.5 text-left',
    'rounded-extra-large overflow-hidden',
    'bg-surface-container-lowest dark:bg-surface-container text-on-surface',
    'transition-[background-color,color,border-radius] duration-150 motion-reduce:transition-none',
  ],
  {
    variants: {
      interactive: {
        true: [
          'cursor-pointer select-none',
          'hover:bg-surface-container-low dark:hover:bg-surface-container-high',
          'active:bg-surface-container dark:active:bg-surface-container-highest',
          'focus-visible:ring-primary focus-visible:ring-2 focus-visible:outline-none focus-visible:ring-inset',
        ],
        false: '',
      },
      selected: {
        true: [
          'bg-secondary-container text-on-secondary-container',
          'hover:bg-secondary-container/90 dark:bg-secondary-container dark:hover:bg-secondary-container/90',
        ],
        false: '',
      },
      destructive: {
        true: 'text-error',
        false: '',
      },
      disabled: {
        true: 'pointer-events-none cursor-not-allowed opacity-38 select-none',
        false: '',
      },
    },
    defaultVariants: {
      interactive: false,
      selected: false,
      destructive: false,
      disabled: false,
    },
  },
)

export type ItemVariantsProps = VariantProps<typeof itemVariants>

export const itemLeadingVariants = cva(
  ['grid shrink-0 place-items-center [&_svg]:size-5'],
  {
    variants: {
      variant: {
        plain: 'text-on-surface-variant size-6',
        tonal:
          'bg-surface-container-highest text-on-surface-variant size-10 rounded-full',
      },
    },
    defaultVariants: {
      variant: 'plain',
    },
  },
)

export type ItemLeadingVariantsProps = VariantProps<typeof itemLeadingVariants>

export const itemSplitSwitchVariants = cva(
  [
    'relative flex min-h-18 w-full min-w-0 items-stretch overflow-hidden',
    'rounded-extra-large',
    'bg-surface-container-lowest dark:bg-surface-container text-on-surface',
    'transition-[background-color,border-radius] duration-150 motion-reduce:transition-none',
  ],
  {
    variants: {
      disabled: {
        true: 'pointer-events-none cursor-not-allowed opacity-38 select-none',
        false: '',
      },
      actionInteractive: {
        true: 'hover:bg-surface-container-low dark:hover:bg-surface-container-high cursor-pointer',
        false: 'cursor-not-allowed',
      },
    },
    compoundVariants: [
      {
        disabled: true,
        actionInteractive: true,
        className:
          'cursor-not-allowed hover:bg-surface-container-lowest dark:hover:bg-surface-container',
      },
    ],
    defaultVariants: {
      disabled: false,
      actionInteractive: false,
    },
  },
)

export const itemSplitSwitchActionVariants = cva(
  [
    'relative z-1 flex min-w-0 flex-1 items-center gap-4 py-3.5 pr-4 pl-5 text-left',
    'transition-colors duration-150 motion-reduce:transition-none',
  ],
  {
    variants: {
      disabled: {
        true: 'cursor-not-allowed select-none',
        false: [
          'cursor-pointer select-none',
          'focus-visible:ring-primary focus-visible:ring-2 focus-visible:outline-none focus-visible:ring-inset',
        ],
      },
    },
    defaultVariants: {
      disabled: false,
    },
  },
)

export function ItemSection({
  className,
  ...props
}: ComponentProps<'section'>) {
  return (
    <section
      data-slot="item-section"
      className={cn(itemSectionVariants(), className)}
      {...props}
    />
  )
}

export function ItemSectionHeader({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      data-slot="item-section-header"
      className={cn(itemSectionHeaderVariants(), className)}
      {...props}
    />
  )
}

export function ItemSectionTitle({
  asChild,
  className,
  ...props
}: ComponentProps<'h2'> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : 'h2'

  return (
    <Comp
      data-slot="item-section-title"
      className={cn(itemSectionTitleVariants(), className)}
      {...props}
    />
  )
}

export function ItemSectionDescription({
  className,
  ...props
}: ComponentProps<'p'>) {
  return (
    <p
      data-slot="item-section-description"
      className={cn(itemSectionDescriptionVariants(), className)}
      {...props}
    />
  )
}

export function ItemGroup({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="item-group"
      className={cn(itemGroupVariants(), className)}
      {...props}
    />
  )
}

export interface ItemProps extends ComponentProps<'div'>, ItemVariantsProps {
  asChild?: boolean
}

export function Item({
  asChild,
  interactive,
  selected,
  destructive,
  disabled,
  className,
  ...props
}: ItemProps) {
  const Comp = asChild ? Slot.Root : 'div'

  return (
    <Comp
      data-slot="item"
      data-selected={selected ? '' : undefined}
      data-disabled={disabled ? '' : undefined}
      className={cn(
        itemVariants({ interactive, selected, destructive, disabled }),
        className,
      )}
      {...props}
    />
  )
}

export function ItemLeading({
  variant,
  className,
  ...props
}: ComponentProps<'div'> & ItemLeadingVariantsProps) {
  return (
    <div
      data-slot="item-leading"
      aria-hidden
      className={cn(itemLeadingVariants({ variant }), className)}
      {...props}
    />
  )
}

export function ItemContent({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="item-content"
      className={cn(
        'flex min-w-0 flex-1 flex-col gap-0.5 text-left',
        className,
      )}
      {...props}
    />
  )
}

export function ItemTitle({ className, ...props }: ComponentProps<'p'>) {
  return (
    <p
      data-slot="item-title"
      className={cn('text-base leading-snug font-medium', className)}
      {...props}
    />
  )
}

export function ItemDescription({ className, ...props }: ComponentProps<'p'>) {
  return (
    <p
      data-slot="item-description"
      className={cn('text-on-surface-variant text-sm leading-snug', className)}
      {...props}
    />
  )
}

export function ItemValue({ className, ...props }: ComponentProps<'span'>) {
  return (
    <span
      data-slot="item-value"
      className={cn(
        'text-on-surface-variant max-w-[45%] min-w-0 shrink-0 truncate text-sm',
        className,
      )}
      {...props}
    />
  )
}

export function ItemTrailing({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="item-trailing"
      className={cn(
        'text-on-surface-variant flex shrink-0 items-center gap-2',
        className,
      )}
      {...props}
    />
  )
}

export interface ItemActionProps extends Omit<
  ComponentProps<'button'>,
  'title' | 'value'
> {
  leading?: ReactNode
  leadingVariant?: ItemLeadingVariantsProps['variant']
  title: ReactNode
  description?: ReactNode
  /** Current value summary shown before the trailing indicator. */
  value?: ReactNode
  /** Custom trailing slot; defaults to a chevron when `showChevron` is true. */
  trailing?: ReactNode
  showChevron?: boolean
  selected?: boolean
  destructive?: boolean
  asChild?: boolean
}

export function ItemAction({
  leading,
  leadingVariant = 'plain',
  title,
  description,
  value,
  trailing,
  showChevron = true,
  selected,
  destructive,
  disabled,
  asChild,
  className,
  children,
  onClick,
  ...props
}: ItemActionProps) {
  const ripple = useRipple()

  const resolvedTrailing =
    trailing !== undefined ? (
      trailing
    ) : showChevron ? (
      <ChevronRightRounded
        className="text-on-surface-variant size-5 shrink-0"
        aria-hidden
      />
    ) : null

  const rowContent = (
    <>
      {leading != null && (
        <ItemLeading variant={leadingVariant} className="relative z-1">
          {leading}
        </ItemLeading>
      )}

      <ItemContent className="relative z-1">
        <ItemTitle>{title}</ItemTitle>

        {description != null && (
          <ItemDescription className={cn(destructive && 'text-error/80')}>
            {description}
          </ItemDescription>
        )}
      </ItemContent>

      {value != null && <ItemValue className="relative z-1">{value}</ItemValue>}

      {resolvedTrailing != null && (
        <ItemTrailing className="relative z-1">{resolvedTrailing}</ItemTrailing>
      )}

      {!disabled && (
        <Ripple
          ripples={ripple.ripples}
          onClear={(key) => ripple.onClear(key)}
        />
      )}
    </>
  )

  const handleClick = (event: MouseEvent<HTMLElement>) => {
    if (disabled) {
      event.preventDefault()
      event.stopPropagation()
      return
    }

    if (event.detail > 0) {
      ripple.onClick(event)
    }
    onClick?.(event as MouseEvent<HTMLButtonElement>)
  }

  if (
    asChild &&
    isValidElement<PropsWithChildren<{ className?: string }>>(children)
  ) {
    return (
      <Slot.Root
        data-slot="item-action"
        data-selected={selected ? '' : undefined}
        data-disabled={disabled ? '' : undefined}
        aria-current={selected ? 'true' : undefined}
        aria-disabled={disabled || undefined}
        tabIndex={disabled ? -1 : props.tabIndex}
        onClick={handleClick}
        className={cn(
          itemVariants({
            interactive: !disabled,
            selected,
            destructive,
            disabled,
          }),
          className,
        )}
        {...props}
      >
        {cloneElement(children, {}, rowContent)}
      </Slot.Root>
    )
  }

  return (
    <button
      type="button"
      data-slot="item-action"
      data-selected={selected ? '' : undefined}
      data-disabled={disabled ? '' : undefined}
      aria-current={selected ? 'true' : undefined}
      disabled={disabled}
      onClick={disabled ? undefined : handleClick}
      className={cn(
        itemVariants({
          interactive: !disabled,
          selected,
          destructive,
          disabled,
        }),
        className,
      )}
      {...props}
    >
      {rowContent}
      {children}
    </button>
  )
}

export interface ItemSwitchProps extends Omit<ComponentProps<'div'>, 'title'> {
  leading?: ReactNode
  leadingVariant?: ItemLeadingVariantsProps['variant']
  title: ReactNode
  description?: ReactNode
  checked?: boolean
  defaultChecked?: boolean
  onCheckedChange?: (checked: boolean) => void
  loading?: boolean
  disabled?: boolean
  switchLabel?: string
}

export function ItemSwitch({
  leading,
  leadingVariant = 'plain',
  title,
  description,
  checked,
  defaultChecked,
  onCheckedChange,
  loading,
  disabled,
  switchLabel,
  className,
  onClick,
  ...props
}: ItemSwitchProps) {
  const switchId = useId()
  const titleId = useId()
  const descriptionId = useId()
  const ripple = useRipple()

  const isInert = Boolean(disabled || loading)

  return (
    <Item
      data-slot="item-switch"
      disabled={disabled}
      onClick={(event) => {
        if (!isInert && event.detail > 0) {
          ripple.onClick(event)
        }
        onClick?.(event)
      }}
      className={cn(
        'p-0',
        isInert
          ? 'cursor-not-allowed'
          : 'hover:bg-surface-container-low dark:hover:bg-surface-container-high cursor-pointer',
        className,
      )}
      {...props}
    >
      <label
        htmlFor={switchId}
        className={cn(
          'relative z-1 flex min-h-16 w-full min-w-0 items-center gap-4 px-5 py-3.5 text-left select-none',
          isInert ? 'cursor-not-allowed' : 'cursor-pointer',
        )}
      >
        {leading != null && (
          <ItemLeading variant={leadingVariant}>{leading}</ItemLeading>
        )}

        <ItemContent>
          <ItemTitle id={titleId}>{title}</ItemTitle>

          {description != null && (
            <ItemDescription id={descriptionId}>{description}</ItemDescription>
          )}
        </ItemContent>

        <ItemTrailing>
          <Switch
            id={switchId}
            checked={checked}
            defaultChecked={defaultChecked}
            onCheckedChange={onCheckedChange}
            loading={loading}
            disabled={isInert}
            aria-label={switchLabel}
            aria-labelledby={switchLabel ? undefined : titleId}
            aria-describedby={description != null ? descriptionId : undefined}
          />
        </ItemTrailing>
      </label>

      {!isInert && (
        <Ripple
          ripples={ripple.ripples}
          onClear={(key) => ripple.onClear(key)}
        />
      )}
    </Item>
  )
}

export interface ItemSplitSwitchProps extends Omit<
  ComponentProps<'div'>,
  'title' | 'value'
> {
  leading?: ReactNode
  leadingVariant?: ItemLeadingVariantsProps['variant']
  title: ReactNode
  description?: ReactNode
  /** Current value summary shown before the detail chevron. */
  value?: ReactNode
  checked?: boolean
  defaultChecked?: boolean
  onCheckedChange?: (checked: boolean) => void
  loading?: boolean
  /** Disables both the detail entry and the switch. */
  disabled?: boolean
  /** Disables only the detail entry while leaving the switch interactive. */
  actionDisabled?: boolean
  /**
   * Allow entering the detail configuration even when the switch is off.
   * Defaults to `false`, matching Android 17 two-target switch rows.
   */
  allowActionWhenUnchecked?: boolean
  onAction?: MouseEventHandler<HTMLButtonElement>
  /** Render the left detail target as its child element (e.g. a router Link or DialogTrigger). */
  actionAsChild?: boolean
  /** Accessible label override for the switch control. */
  switchLabel?: string
}

export function ItemSplitSwitch({
  leading,
  leadingVariant = 'tonal',
  title,
  description,
  value,
  checked,
  defaultChecked,
  onCheckedChange,
  loading,
  disabled,
  actionDisabled,
  allowActionWhenUnchecked = false,
  onAction,
  actionAsChild,
  switchLabel,
  className,
  children,
  onClick,
  ...props
}: ItemSplitSwitchProps) {
  const switchId = useId()
  const titleId = useId()
  const descriptionId = useId()
  const actionRef = useRef<HTMLElement>(null)
  const ripple = useRipple()

  const isSwitchInert = Boolean(disabled || loading)

  const [isChecked = false, setIsChecked] = useControllableState({
    prop: checked,
    defaultProp: defaultChecked ?? false,
    onChange: onCheckedChange,
  })

  const isActionDisabled = Boolean(
    disabled || actionDisabled || (!allowActionWhenUnchecked && !isChecked),
  )

  const actionContent = (
    <>
      {leading != null && (
        <ItemLeading variant={leadingVariant}>{leading}</ItemLeading>
      )}

      <ItemContent>
        <ItemTitle id={titleId}>{title}</ItemTitle>

        {description != null && (
          <ItemDescription id={descriptionId}>{description}</ItemDescription>
        )}
      </ItemContent>

      {value != null && <ItemValue>{value}</ItemValue>}

      <ChevronRightRounded
        className={cn(
          'text-on-surface-variant size-5 shrink-0 transition-opacity duration-150',
          isActionDisabled && 'opacity-38',
        )}
        aria-hidden
      />
    </>
  )

  const handleChildActionClick = (event: MouseEvent<HTMLElement>) => {
    if (isActionDisabled) {
      event.preventDefault()
      event.stopPropagation()
      return
    }

    onAction?.(event as MouseEvent<HTMLButtonElement>)
  }

  const handleRootClick = (event: MouseEvent<HTMLDivElement>) => {
    if (disabled) {
      event.preventDefault()
      event.stopPropagation()
      return
    }

    const target = event.target instanceof Element ? event.target : null
    const hitSwitch = Boolean(target?.closest('[data-slot="switch"]'))

    if (hitSwitch) {
      onClick?.(event)
      return
    }

    if (!isActionDisabled) {
      if (event.detail > 0) {
        ripple.onClick(event)
      }

      const hitAction = Boolean(
        target?.closest('[data-slot="item-split-switch-action"]'),
      )

      if (!hitAction) {
        actionRef.current?.click()
      }
    }

    onClick?.(event)
  }

  return (
    <div
      data-slot="item-split-switch"
      data-state={isChecked ? 'checked' : 'unchecked'}
      data-disabled={disabled ? '' : undefined}
      data-action-disabled={isActionDisabled ? '' : undefined}
      onClick={handleRootClick}
      className={cn(
        itemSplitSwitchVariants({
          disabled,
          actionInteractive: !isActionDisabled,
        }),
        className,
      )}
      {...props}
    >
      {actionAsChild &&
      isValidElement<PropsWithChildren<{ className?: string }>>(children) ? (
        <Slot.Root
          ref={actionRef}
          data-slot="item-split-switch-action"
          data-disabled={isActionDisabled ? '' : undefined}
          aria-disabled={isActionDisabled || undefined}
          tabIndex={isActionDisabled ? -1 : undefined}
          onClick={handleChildActionClick}
          className={itemSplitSwitchActionVariants({
            disabled: isActionDisabled,
          })}
        >
          {cloneElement(children, {}, actionContent)}
        </Slot.Root>
      ) : (
        <button
          ref={actionRef as RefObject<HTMLButtonElement | null>}
          type="button"
          data-slot="item-split-switch-action"
          data-disabled={isActionDisabled ? '' : undefined}
          disabled={isActionDisabled}
          onClick={isActionDisabled ? undefined : onAction}
          className={itemSplitSwitchActionVariants({
            disabled: isActionDisabled,
          })}
        >
          {actionContent}
        </button>
      )}

      <div
        data-slot="item-split-switch-divider"
        aria-hidden
        className="bg-outline-variant relative z-1 my-auto h-8 w-px shrink-0"
      />

      <div
        data-slot="item-split-switch-control"
        className="relative z-1 flex shrink-0 items-center self-stretch pr-5 pl-4 select-none"
      >
        <Switch
          id={switchId}
          checked={isChecked}
          onCheckedChange={setIsChecked}
          loading={loading}
          disabled={isSwitchInert}
          aria-label={switchLabel}
          aria-labelledby={switchLabel ? undefined : titleId}
          aria-describedby={description != null ? descriptionId : undefined}
        />
      </div>

      {!disabled && (
        <Ripple
          ripples={ripple.ripples}
          onClear={(key) => ripple.onClear(key)}
        />
      )}
    </div>
  )
}
