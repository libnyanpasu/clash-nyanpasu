import FlashOnRounded from '~icons/material-symbols/flash-on-rounded'
import { ComponentProps, MouseEvent, useMemo } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { ClashProxiesQueryProxyItem } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'

export default function ProxyNodeButton({
  proxy,
  ...props
}: Omit<ComponentProps<typeof Button>, 'onClick' | 'children'> & {
  proxy: ClashProxiesQueryProxyItem
}) {
  const handleSelectProxy = useLockFn(async () => {
    await proxy.mutateSelect()
  })

  const delayTask = useBlockTask(
    `proxy-delay-check-${proxy.name.toLowerCase()}`,
    async () => {
      await proxy.mutateDelay()
    },
  )

  const handleDelayClick = useLockFn(
    async (e: MouseEvent<HTMLButtonElement>) => {
      e.preventDefault()
      e.stopPropagation()

      await delayTask.execute()
    },
  )

  const currentDelay = useMemo(() => {
    if (!proxy.history || proxy.history.length === 0) {
      return -1
    } else {
      return proxy.history[proxy.history.length - 1].delay
    }
  }, [proxy.history])

  return (
    <Button
      variant="fab"
      className={cn(
        'flex w-full flex-col justify-center gap-1 px-2 text-left',
        'group-data-[active=true]:bg-surface-variant/50',
        'group-data-[active=false]:bg-surface/30',
        'group-data-[active=false]:shadow-none',
        'group-data-[active=false]:hover:shadow-none',
        'group-data-[active=false]:hover:bg-surface-variant/30',
      )}
      onClick={handleSelectProxy}
      {...props}
    >
      <div className="flex items-center gap-2 px-2">
        <div className="truncate text-sm font-medium">{proxy.name}</div>
      </div>

      <div className="flex items-center gap-2">
        <div className="flex-1" />

        <Button
          className="grid h-4 min-w-10 place-content-center px-2 text-center"
          variant="raised"
          onClick={handleDelayClick}
          loading={delayTask.isPending}
          asChild
        >
          {currentDelay > 0 ? (
            <span
              className={cn(
                'text-[10px]',
                currentDelay > 0 && 'text-green-500!',
                currentDelay > 100 && 'text-yellow-500!',
                currentDelay > 300 && 'text-orange-500!',
                currentDelay > 500 && 'text-red-500!',
              )}
            >
              {currentDelay} ms
            </span>
          ) : (
            <span>
              <FlashOnRounded className="py-1" />
            </span>
          )}
        </Button>
      </div>
    </Button>
  )
}
