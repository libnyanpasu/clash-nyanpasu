import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { Button } from '@nyanpasu/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { useLocation, useRouter } from '@tanstack/react-router'
import { returnStep, type CrossPage } from './cross-navigation'

const pageLabel = (page: CrossPage) => {
  switch (page) {
    case 'rules':
      return m.navbar_label_rules()
    case 'connections':
      return m.navbar_label_connections()
    case 'traffic':
      return m.topology_title()
  }
}

/** Returns to the entry that jumped here; renders nothing without a ticket. */
export function ReturnButton({ className }: { className?: string }) {
  const router = useRouter()

  const ticket = useLocation({ select: (location) => location.state.returnTo })

  if (!ticket) {
    return null
  }

  const label = m.navigation_return_to({ page: pageLabel(ticket.page) })

  const handleClick = () => {
    const step = returnStep(ticket, router.history.location.state.__TSR_index)

    if (step.kind === 'go') {
      router.history.go(step.delta)
    } else {
      router.navigate({ href: step.href })
    }
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          className={cn('shrink-0', className)}
          data-slot="return-button"
          aria-label={label}
          onClick={handleClick}
          icon
        >
          <ArrowBackIosNewRounded className="size-4" />
        </Button>
      </TooltipTrigger>

      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}
