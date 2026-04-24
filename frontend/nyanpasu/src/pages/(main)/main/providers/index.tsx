import AllInboxRounded from '~icons/material-symbols/all-inbox-outline-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import dayjs from 'dayjs'
import { filesize } from 'filesize'
import { ComponentProps, PropsWithChildren } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { LinearProgress } from '@/components/ui/progress'
import TextMarquee from '@/components/ui/text-marquee'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  ClashProxiesProviderQueryItem,
  ClashRulesProviderQueryItem,
  useClashProxiesProvider,
  useClashRulesProvider,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useProxiesProviderUpdate } from './_modules/use-proxies-provider-update'
import { useProxiesSubscription } from './_modules/use-proxies-subscription'
import { useRulesProviderUpdate } from './_modules/use-rules-provider-update'

export const Route = createFileRoute('/(main)/main/providers/')({
  component: RouteComponent,
})

const NavigateButton = ({
  className,
  ...props
}: ComponentProps<typeof Button>) => {
  return (
    <Button
      variant="fab"
      className={cn(
        'flex h-auto w-full flex-col justify-center gap-1 p-3 text-left',
        'bg-on-background/3!',
        'dark:bg-surface!',
        'shadow-none',
        'hover:shadow-none',
        'hover:bg-surface-variant/30',
        className,
      )}
      asChild
      {...props}
    />
  )
}

const Group = ({ children }: PropsWithChildren) => {
  return (
    <div className="flex flex-col gap-1" data-slot="providers-group">
      {children}
    </div>
  )
}

const GroupTitle = ({ children }: PropsWithChildren) => {
  return (
    <div
      className={cn(
        'sticky top-0 z-10 pl-1 text-lg font-semibold',
        'text-secondary bg-mixed-background flex h-16 items-center justify-between',
      )}
      data-slot="providers-group-title"
    >
      {children}
    </div>
  )
}

const GroupContent = ({ children }: PropsWithChildren) => {
  return (
    <div
      className="grid grid-cols-2 gap-2 sm:grid-cols-3 md:grid-cols-4"
      data-slot="providers-group-content"
    >
      {children}
    </div>
  )
}

const Empty = ({ children }: PropsWithChildren) => {
  return (
    <Card variant="outline">
      <CardContent className="min-h-40 items-center justify-center text-sm">
        <AllInboxRounded className="size-10" />

        {children}
      </CardContent>
    </Card>
  )
}

const Proxies = ({ data }: { data: ClashProxiesProviderQueryItem }) => {
  const { progress, total, used, hasSubscriptionInfo } =
    useProxiesSubscription(data)

  const blockTask = useProxiesProviderUpdate(data)

  const handleClick = useLockFn(blockTask.execute)

  return (
    <NavigateButton className="flex flex-col gap-2">
      <Link
        to="/main/providers/proxies/$key"
        params={{
          key: data.name,
        }}
      >
        <div className="flex items-center justify-between gap-2">
          <TextMarquee className="text-sm font-medium">{data.name}</TextMarquee>

          <div className="text-xs text-nowrap text-zinc-700 dark:text-zinc-300">
            {dayjs(data.updatedAt).fromNow()}
          </div>
        </div>

        <div className="text-xs text-zinc-500">
          {data.vehicleType}/{data.type}
        </div>

        <div className="flex flex-1 flex-col gap-2 text-xs text-zinc-500">
          {hasSubscriptionInfo && (
            <>
              <LinearProgress value={progress} />

              <TextMarquee>
                <div className="flex items-center justify-between gap-2 text-xs font-bold">
                  <div>{progress.toFixed(2)}%</div>

                  <div>
                    {filesize(used)} / {filesize(total)}
                  </div>
                </div>
              </TextMarquee>
            </>
          )}
        </div>

        <div className="flex items-center justify-between">
          <div className="bg-surface-variant text-secondary rounded-full px-2 py-1 text-[10px]">
            {m.providers_proxies_proxy_count_label({
              count: data.proxies.length,
            })}
          </div>

          <Button
            className="size-6"
            icon
            loading={blockTask.isPending}
            onClick={(e) => {
              e.preventDefault()
              e.stopPropagation()
              handleClick()
            }}
          >
            <RefreshRounded />
          </Button>
        </div>
      </Link>
    </NavigateButton>
  )
}

const Rules = ({ data }: { data: ClashRulesProviderQueryItem }) => {
  const blockTask = useRulesProviderUpdate(data)

  const handleClick = useLockFn(blockTask.execute)

  return (
    <NavigateButton className="flex flex-col gap-2">
      <Link
        to="/main/providers/rules/$key"
        params={{
          key: data.name,
        }}
      >
        <div className="flex items-center justify-between gap-2">
          <TextMarquee className="text-sm font-medium">{data.name}</TextMarquee>

          <div className="text-xs text-nowrap text-zinc-700 dark:text-zinc-300">
            {dayjs(data.updatedAt).fromNow()}
          </div>
        </div>

        <div className="text-xs text-zinc-500">
          {data.vehicleType}/{data.type}
        </div>

        <div className="flex items-center justify-between">
          <div className="bg-surface-variant text-secondary rounded-full px-2 py-1 text-[10px]">
            {m.providers_rules_rule_count_label({
              count: data.ruleCount,
            })}
          </div>

          <Button
            className="size-6"
            icon
            loading={blockTask.isPending}
            onClick={(e) => {
              e.preventDefault()
              e.stopPropagation()
              handleClick()
            }}
          >
            <RefreshRounded />
          </Button>
        </div>
      </Link>
    </NavigateButton>
  )
}

function RouteComponent() {
  const proxiesProvider = useClashProxiesProvider()

  const proxies = proxiesProvider.data
    ? Object.entries(proxiesProvider.data)
    : null

  const proxiesBlockTask = useBlockTask('update-proxies-provider', async () => {
    if (!proxies) {
      return
    }

    try {
      await Promise.all(proxies.map(([_, data]) => data.mutate()))
    } catch (error) {
      console.error('Failed to update proxies provider', error)
      message(`Update provider failed: \n ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  const handleUpdateProxies = useLockFn(proxiesBlockTask.execute)

  const rulesProvider = useClashRulesProvider()

  const rules = rulesProvider.data ? Object.entries(rulesProvider.data) : null

  const rulesBlockTask = useBlockTask('update-rules-provider', async () => {
    if (!rules) {
      return
    }

    try {
      await Promise.all(rules.map(([_, data]) => data.mutate()))
    } catch (error) {
      console.error('Failed to update rules provider', error)
      message(`Update provider failed: \n ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  const handleUpdateRules = useLockFn(rulesBlockTask.execute)

  return (
    <div className="flex flex-col gap-4 p-4 pt-0">
      <Group>
        <GroupTitle>
          <span>{m.providers_proxies_title()}</span>

          <Button
            icon
            onClick={handleUpdateProxies}
            loading={proxiesBlockTask.isPending}
          >
            <RefreshRounded />
          </Button>
        </GroupTitle>

        {proxies && proxies.length ? (
          <GroupContent>
            {proxies.map(([key, data]) => (
              <Proxies key={key} data={data} />
            ))}
          </GroupContent>
        ) : (
          <Empty>
            <p>{m.providers_no_proxies_message()}</p>
          </Empty>
        )}
      </Group>

      <Group>
        <GroupTitle>
          <span>{m.providers_rules_title()}</span>

          <Button
            icon
            onClick={handleUpdateRules}
            loading={rulesBlockTask.isPending}
          >
            <RefreshRounded />
          </Button>
        </GroupTitle>

        {rules && rules.length ? (
          <GroupContent>
            {rules.map(([key, data]) => (
              <Rules key={key} data={data} />
            ))}
          </GroupContent>
        ) : (
          <Empty>
            <p>{m.providers_no_rules_message()}</p>
          </Empty>
        )}
      </Group>
    </div>
  )
}
