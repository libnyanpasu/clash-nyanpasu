import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { ComponentProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { useSidebarContext } from '@nyanpasu/ui/sidebar'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'

const BackButton = () => {
  const { isHiddenSide } = useSidebarContext()

  if (!isHiddenSide) {
    return null
  }

  return (
    <Button icon className="flex items-center justify-center" asChild>
      <Link to="/main/proxies" search={(previous) => ({ q: previous.q })}>
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
    // The horizontal padding sits on the root so the header row and the
    // content under it share their edges, also while the scrollbar shows.
    <div
      className={cn(
        'sticky top-0 z-10 transition-[padding] duration-500',
        'bg-mixed-background',
        'flex flex-col',
        'pr-4 pl-2 md:pl-4',
        'group-data-[scroll-direction=down]/proxies-content:pr-6',
        'group-data-[scroll-direction=down]/proxies-content:pl-3',
        'group-data-[scroll-direction=down]/proxies-content:md:pl-6',
        className,
      )}
      {...props}
    >
      <div className="flex items-center gap-1 py-2 md:py-4">
        <BackButton />

        {children}
      </div>
    </div>
  )
}
