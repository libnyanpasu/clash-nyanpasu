import BoltRounded from '~icons/material-symbols/bolt-rounded'
import { useState } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { sleep } from '@/utils'
import { useClashProxies } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { Route as NameRoute } from '../$name'

export default function DelayTestButton() {
  const { name } = NameRoute.useParams()

  const { updateGroupDelay } = useClashProxies()

  const [isSuccess, setIsSuccess] = useState(false)

  const blockTask = useBlockTask(`delay-group-test-${name}`, async () => {
    await updateGroupDelay.mutateAsync([name])
  })

  const handleClick = useLockFn(async () => {
    await blockTask.execute()

    // success effect
    setIsSuccess(true)
    await sleep(1000)
    setIsSuccess(false)
  })

  return (
    <div
      data-slot="delay-test-button"
      data-success={String(isSuccess)}
      data-loading={String(blockTask.isPending)}
      className={cn(
        'absolute right-4 ml-auto w-fit',
        // The top position is calculated based on the viewport height and the heights of other components (header, tabs, etc.)
        // DO NOT change these values unless you know what you are doing
        'top-[calc(100vh-40px-64px-72px)]',
        'sm:top-[calc(100vh-40px-48px-72px)]',
        'transition-[top] duration-500 sm:bottom-4',
        'data-[loading=false]:data-[success=false]:group-data-[bottom=true]/proxies-content:top-full',
      )}
    >
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            data-slot="delay-test-button-trigger"
            data-success={String(isSuccess)}
            data-loading={String(blockTask.isPending)}
            className={cn(
              "**:data-[slot='circular-progress']:size-6",
              'transition-colors',
              'backdrop-blur',
              'data-[loading=false]:bg-primary-container/35',
              'data-[loading=false]:dark:bg-on-primary/35',
              'data-[success=true]:bg-green-500/30',
              'data-[success=true]:dark:bg-green-700/50',
            )}
            variant="fab"
            icon
            loading={blockTask.isPending}
            onClick={handleClick}
          >
            <BoltRounded className="size-6" />
          </Button>
        </TooltipTrigger>

        <TooltipContent data-slot="delay-test-button-tooltip">
          <span>
            {blockTask.isPending
              ? m.proxies_group_delay_test_pending_title()
              : m.proxies_group_delay_test_title()}
          </span>
        </TooltipContent>
      </Tooltip>
    </div>
  )
}
