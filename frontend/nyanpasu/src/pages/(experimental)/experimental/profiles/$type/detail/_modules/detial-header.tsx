import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@nyanpasu/ui'
import { Link } from '@tanstack/react-router'
import { Route } from '../$uid'

const BackButton = () => {
  const { type } = Route.useParams()

  return (
    <Button icon className="flex items-center justify-center" asChild>
      <Link to={`/experimental/profiles/${type}`}>
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  )
}

export default function DetialHeader({
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
        'py-2 pr-4 pl-2',
        'group-data-[scroll-direction=down]/proxies-content:pr-6',
        'group-data-[scroll-direction=down]/proxies-content:pl-3',
        className,
      )}
      {...props}
    >
      <BackButton />

      {children}
    </div>
  )
}
