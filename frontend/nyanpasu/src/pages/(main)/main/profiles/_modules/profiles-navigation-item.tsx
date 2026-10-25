import { ForwardedRef, ReactNode } from 'react'
import {
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
  type NavigationListItemColor,
} from '@nyanpasu/ui/navigation-list'
import {
  useLinkProps,
  type LinkComponentProps,
  type RegisteredRouter,
} from '@tanstack/react-router'

export default function ProfilesNavigationItem<
  const TFrom extends string = string,
  const TTo extends string | undefined = undefined,
  const TMaskFrom extends string = TFrom,
  const TMaskTo extends string = '',
>({
  icon,
  color,
  label,
  description,
  ref,
  ...props
}: LinkComponentProps<'a', RegisteredRouter, TFrom, TTo, TMaskFrom, TMaskTo> & {
  icon: ReactNode
  color: NavigationListItemColor
  label: string
  description: string
}) {
  // An item stays selected on its child pages, such as a profile's detail
  // page. The link tells whether it is active from the location being
  // navigated to; `matchRoute` reads the last resolved one until the
  // navigation settles, so a page opened from another section would render
  // its item unselected first and then fade into the selected colors.
  const linkProps = useLinkProps(
    {
      ...props,
      activeOptions: { includeSearch: false },
    } as Parameters<typeof useLinkProps>[0],
    ref as ForwardedRef<Element>,
  )
  const isActive = linkProps['aria-current'] === 'page'

  return (
    <NavigationListItem active={isActive} asChild>
      <a {...linkProps}>
        <NavigationListItemIcon color={color}>{icon}</NavigationListItemIcon>

        <NavigationListItemText headline={label} supporting={description} />
      </a>
    </NavigationListItem>
  )
}
