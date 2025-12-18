import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@nyanpasu/ui'
import { Link } from '@tanstack/react-router'

const BackButton = () => {
  return (
    <Button icon className="flex items-center justify-center sm:hidden" asChild>
      <Link to="/experimental/settings">
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  )
}

export function SettingsTitlePlaceholder({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('h-4', className)}
      data-slot="settings-title-placeholder"
      {...props}
    />
  )
}

export function SettingsTitle({
  className,
  children,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'sticky top-0 z-10 transition-[padding] duration-500',
        'backdrop-blur-xl',
        'flex items-center gap-1',
        'py-4 pr-4 pl-2 sm:pl-4',
        'group-data-[scroll-direction=down]/settings-content:pr-6',
        'group-data-[scroll-direction=down]/settings-content:pl-3',
        'group-data-[scroll-direction=down]/settings-content:sm:pl-6',
        className,
      )}
      data-slot="settings-title"
      {...props}
    >
      <BackButton />

      <p className="text-2xl font-bold">{children}</p>
    </div>
  )
}
