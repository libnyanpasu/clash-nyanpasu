import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import {
  cloneElement,
  ComponentProps,
  isValidElement,
  PropsWithChildren,
  ReactNode,
  useId,
} from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Switch } from '@nyanpasu/ui/switch'
import { cn } from '@nyanpasu/utils'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'

/** A titled group of rows on the detail page. */
export function DetailSection({
  title,
  description,
  action,
  className,
  children,
  ...props
}: Omit<ComponentProps<'section'>, 'title'> & {
  title: ReactNode
  /** Explains the group, such as what editing it affects. */
  description?: ReactNode
  /** Sits at the end of the heading, such as a count. */
  action?: ReactNode
}) {
  const headingId = useId()

  return (
    <section
      aria-labelledby={headingId}
      data-slot="profile-detail-section"
      className={className}
      {...props}
    >
      <SettingsLabel className="flex items-center justify-between gap-2">
        <h2 id={headingId}>{title}</h2>

        {action}
      </SettingsLabel>

      {description && (
        <p className="text-on-surface-variant px-4 pb-2 text-sm">
          {description}
        </p>
      )}

      <SettingsGroup>{children}</SettingsGroup>
    </section>
  )
}

/**
 * One tappable row. It forwards its props to the button so a modal or menu
 * trigger can take it as the child.
 */
export function DetailRow({
  label,
  description,
  value,
  leading,
  trailing = (
    <ChevronRightRounded
      className="text-on-surface-variant size-5 shrink-0"
      aria-hidden
    />
  ),
  destructive,
  asChild,
  className,
  children,
  ...props
}: Omit<ComponentProps<typeof Button>, 'children' | 'label'> & {
  label: ReactNode
  description?: ReactNode
  /** The current setting, shown before the trailing icon. */
  value?: ReactNode
  leading?: ReactNode
  trailing?: ReactNode
  destructive?: boolean
  // Triggers hand their own layers over as children. With `asChild` the
  // child is the element that carries the row, such as a link.
  children?: ReactNode
}) {
  const row = (
    <ItemContainer>
      {leading}

      <ItemLabel className="min-w-0 flex-1">
        <ItemLabelText>{label}</ItemLabelText>

        {description && (
          <ItemLabelDescription className={cn(destructive && 'text-error/80')}>
            {description}
          </ItemLabelDescription>
        )}
      </ItemLabel>

      {value && (
        <span
          data-slot="profile-detail-row-value"
          className="text-on-surface-variant max-w-[55%] min-w-0 truncate text-sm font-normal"
        >
          {value}
        </span>
      )}

      {trailing}
    </ItemContainer>
  )

  return (
    <SettingsCard>
      <SettingsCardContent asChild>
        <Button
          data-slot="profile-detail-row"
          asChild={asChild}
          className={cn(
            'h-auto w-full rounded-none px-5 text-left text-base',
            destructive ? 'text-error!' : 'text-on-surface!',
            className,
          )}
          {...props}
        >
          {asChild && isValidElement<PropsWithChildren>(children) ? (
            cloneElement(children, {}, row)
          ) : (
            <>
              {row}

              {children}
            </>
          )}
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  )
}

/** One row with an inline switch, for a setting that applies as soon as it flips. */
export function DetailSwitchRow({
  label,
  description,
  checked,
  onCheckedChange,
  loading,
}: {
  label: ReactNode
  description?: ReactNode
  checked: boolean
  onCheckedChange: (checked: boolean) => void
  loading?: boolean
}) {
  const labelId = useId()

  return (
    <SettingsCard data-slot="profile-detail-switch-row">
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel className="min-w-0 flex-1">
            <ItemLabelText id={labelId}>{label}</ItemLabelText>

            {description && (
              <ItemLabelDescription>{description}</ItemLabelDescription>
            )}
          </ItemLabel>

          <Switch
            aria-labelledby={labelId}
            checked={checked}
            onCheckedChange={onCheckedChange}
            loading={loading}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  )
}
