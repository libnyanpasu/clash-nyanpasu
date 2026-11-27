import { ComponentProps, ReactNode, useId } from 'react'
import {
  ItemAction,
  ItemGroup,
  ItemSection,
  ItemSectionDescription,
  ItemSectionHeader,
  ItemSectionTitle,
  ItemSwitch,
  type ItemActionProps,
} from '@nyanpasu/ui/item-group'
import { CircularProgress } from '@nyanpasu/ui/progress'

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
    <ItemSection
      aria-labelledby={headingId}
      data-slot="profile-detail-section"
      className={className}
      {...props}
    >
      <ItemSectionHeader>
        <ItemSectionTitle id={headingId}>{title}</ItemSectionTitle>

        {action}
      </ItemSectionHeader>

      {description != null && (
        <ItemSectionDescription>{description}</ItemSectionDescription>
      )}

      <ItemGroup>{children}</ItemGroup>
    </ItemSection>
  )
}

type DetailRowProps = Omit<
  ItemActionProps,
  | 'title'
  | 'description'
  | 'value'
  | 'leading'
  | 'trailing'
  | 'destructive'
  | 'asChild'
> & {
  label: ReactNode
  description?: ReactNode
  /** The current setting, shown before the trailing icon. */
  value?: ReactNode
  leading?: ReactNode
  trailing?: ReactNode
  destructive?: boolean
  asChild?: boolean
  loading?: boolean
  children?: ReactNode
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
  trailing,
  destructive,
  loading,
  disabled,
  asChild,
  className,
  children,
  ...props
}: DetailRowProps) {
  return (
    <ItemAction
      data-slot="profile-detail-row"
      asChild={asChild}
      leading={leading}
      title={label}
      description={description}
      value={
        value != null ? (
          <span data-slot="profile-detail-row-value">{value}</span>
        ) : undefined
      }
      trailing={
        loading ? (
          <CircularProgress className="size-5" indeterminate />
        ) : (
          trailing
        )
      }
      destructive={destructive}
      disabled={disabled || loading}
      className={className}
      {...props}
    >
      {children}
    </ItemAction>
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
  return (
    <ItemSwitch
      data-slot="profile-detail-switch-row"
      title={label}
      description={description}
      checked={checked}
      onCheckedChange={onCheckedChange}
      loading={loading}
    />
  )
}
