import { ReactNode } from 'react'
import {
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
  type NavigationListItemColor,
} from '@nyanpasu/ui/navigation-list'
import {
  Link,
  useMatchRoute,
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
  ...props
}: LinkComponentProps<'a', RegisteredRouter, TFrom, TTo, TMaskFrom, TMaskTo> & {
  icon: ReactNode
  color: NavigationListItemColor
  label: string
  description: string
}) {
  const matchRoute = useMatchRoute()

  const linkProps = props as LinkComponentProps<
    'a',
    RegisteredRouter,
    TFrom,
    TTo,
    TMaskFrom,
    TMaskTo
  >

  // An item stays selected on its child pages, such as a profile's detail page.
  const isActive = !!matchRoute({ ...linkProps, fuzzy: true })

  return (
    <NavigationListItem active={isActive} asChild>
      <Link aria-current={isActive ? 'page' : undefined} {...linkProps}>
        <NavigationListItemIcon color={color}>{icon}</NavigationListItemIcon>

        <NavigationListItemText headline={label} supporting={description} />
      </Link>
    </NavigationListItem>
  )
}
