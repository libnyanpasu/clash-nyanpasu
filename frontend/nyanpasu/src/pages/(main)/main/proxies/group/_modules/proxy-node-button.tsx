import AddRounded from '~icons/material-symbols/add-rounded'
import CheckCircleRounded from '~icons/material-symbols/check-circle-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import FlashOnRounded from '~icons/material-symbols/flash-on-rounded'
import RadioButtonUnchecked from '~icons/material-symbols/radio-button-unchecked'
import { ComponentProps, MouseEvent, useMemo } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import DelayChip from '@/components/proxies/delay-chip'
import { Button } from '@/components/ui/button'
import { ContextMenuItem } from '@/components/ui/context-menu'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { ClashProxiesQueryProxyItem } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { useGroupSelection } from './selection'

export default function ProxyNodeButton({
  proxy,
  ...props
}: Omit<ComponentProps<typeof Button>, 'onClick' | 'children'> & {
  proxy: ClashProxiesQueryProxyItem
}) {
  const { selecting, isSelected, toggle, enter, exit, openCreate, count } =
    useGroupSelection()

  const selected = isSelected(proxy.name)

  const handleSelectProxy = useLockFn(async () => {
    if (selecting) {
      toggle(proxy.name)
      return
    }

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
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <Button
          variant="fab"
          className={cn(
            'flex w-full flex-col justify-center gap-1 px-2 text-left',
            'group-data-[active=true]:bg-primary-container/75',
            'dark:group-data-[active=true]:bg-surface-variant/50',
            'group-data-[active=false]:bg-on-background/3',
            'dark:group-data-[active=false]:bg-surface/30',
            'group-data-[active=false]:shadow-none',
            'group-data-[active=false]:hover:shadow-none',
            'group-data-[active=false]:hover:bg-surface-variant/30',
            selected && 'ring-primary ring-2 ring-inset',
          )}
          onClick={handleSelectProxy}
          {...props}
        >
          <div className="flex items-center gap-2 px-2">
            {selecting &&
              (selected ? (
                <CheckCircleRounded className="text-primary size-4 shrink-0" />
              ) : (
                <RadioButtonUnchecked className="text-outline size-4 shrink-0" />
              ))}

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
                <DelayChip delay={currentDelay} />
              ) : (
                <span>
                  <FlashOnRounded className="py-1" />
                </span>
              )}
            </Button>
          </div>
        </Button>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        {!selecting ? (
          <ContextMenuItem onClick={() => enter(proxy.name)}>
            <AddRounded className="size-4" />
            <span>{m.proxies_group_select_nodes()}</span>
          </ContextMenuItem>
        ) : (
          <>
            <ContextMenuItem onClick={() => toggle(proxy.name)}>
              {selected ? (
                <CheckCircleRounded className="size-4" />
              ) : (
                <RadioButtonUnchecked className="size-4" />
              )}
              <span>
                {selected
                  ? m.proxies_group_deselect_node()
                  : m.proxies_group_select_node()}
              </span>
            </ContextMenuItem>

            <ContextMenuItem disabled={count === 0} onClick={openCreate}>
              <AddRounded className="size-4" />
              <span>{m.proxies_group_create_group_action()}</span>
            </ContextMenuItem>

            <ContextMenuItem onClick={exit}>
              <CloseRounded className="size-4" />
              <span>{m.proxies_group_exit_select()}</span>
            </ContextMenuItem>
          </>
        )}
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  )
}
