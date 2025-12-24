import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@nyanpasu/ui'
import { Link } from '@tanstack/react-router'

const BackButton = () => {
  return (
    <Button icon className="flex items-center justify-center md:hidden" asChild>
      <Link to="/experimental/proxies">
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  )
}

export default function GroupHeader({
  children,
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'sticky top-0 z-10 transition-[padding] duration-500',
        'backdrop-blur-xl',
        'flex items-center gap-1',
        'py-2 pr-4 pl-2 md:py-4 md:pl-4',
        'group-data-[scroll-direction=down]/proxies-content:pr-6',
        'group-data-[scroll-direction=down]/proxies-content:pl-3',
        'group-data-[scroll-direction=down]/proxies-content:md:pl-6',
        className,
      )}
      {...props}
    >
      <BackButton />

      {children}
    </div>
  )
}
