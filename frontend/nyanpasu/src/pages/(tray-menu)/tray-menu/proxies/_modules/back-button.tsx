import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { ComponentProps, PropsWithChildren } from 'react'
import { useCrumbParent } from '@/components/router/route-crumbs'
import { Link } from '@tanstack/react-router'
import { ActionButton } from '../../_modules/action-button'

export default function BackButton({
  children,
  ...props
}: PropsWithChildren<Omit<ComponentProps<typeof Link>, 'to' | 'params'>>) {
  const parent = useCrumbParent()

  if (!parent) {
    return null
  }

  return (
    <ActionButton
      className="sticky top-3 z-10 backdrop-blur-lg"
      disableClose
      asChild
    >
      <Link {...props} to={parent.to} params={parent.params as never}>
        <ArrowBackIosNewRounded />

        {children}
      </Link>
    </ActionButton>
  )
}
