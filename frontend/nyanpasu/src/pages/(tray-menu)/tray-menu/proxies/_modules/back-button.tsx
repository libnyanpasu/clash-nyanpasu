import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { ComponentProps, PropsWithChildren } from 'react'
import { Link } from '@tanstack/react-router'
import { ActionButton } from '../../_modules/action-button'

export default function BackButton({
  children,
  ...props
}: PropsWithChildren<ComponentProps<typeof Link>>) {
  return (
    <ActionButton
      className="sticky top-3 z-10 backdrop-blur-lg"
      disableClose
      asChild
    >
      <Link {...props}>
        <ArrowBackIosNewRounded />

        {children}
      </Link>
    </ActionButton>
  )
}
