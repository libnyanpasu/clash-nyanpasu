import NetworkPing from '~icons/material-symbols/network-ping-rounded'
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button, ButtonProps } from '@/components/ui/button'
import { CircularProgress } from '@/components/ui/progress'
import { m } from '@/paraglide/messages'
import { useSetting } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'

const ProxyButton = ({
  className,
  isActive,
  loading,
  children,
  ...props
}: ButtonProps & {
  isActive?: boolean
}) => {
  return (
    <Button
      className={cn(
        'group h-16 rounded-3xl font-bold',
        'flex items-center justify-between gap-2',
        'data-[active=false]:bg-white dark:data-[active=false]:bg-black',
        className,
      )}
      data-active={String(Boolean(isActive))}
      data-loading={String(Boolean(loading))}
      disabled={loading}
      variant="fab"
      {...props}
    >
      <div className="flex items-center gap-3 [&_svg]:size-7">{children}</div>

      {loading && (
        <CircularProgress
          className={cn(
            'size-6 transition-opacity',
            'group-data-[loading=false]:opacity-0 group-data-[loading=true]:opacity-100',
          )}
          indeterminate
        />
      )}
    </Button>
  )
}

export const SystemProxyButton = (
  props: Omit<ButtonProps, 'children' | 'loading'>,
) => {
  const systemProxy = useSetting('enable_system_proxy')

  const { execute, isPending } = useBlockTask('system-proxy', async () => {
    await systemProxy.upsert(!systemProxy.value)
  })

  return (
    <ProxyButton
      {...props}
      loading={isPending}
      onClick={execute}
      isActive={Boolean(systemProxy.value)}
    >
      <NetworkPing />
      <span>{m.settings_system_proxy_system_proxy_label()}</span>
    </ProxyButton>
  )
}

export const TunModeButton = (
  props: Omit<ButtonProps, 'children' | 'loading'>,
) => {
  const tunMode = useSetting('enable_tun_mode')

  const { execute, isPending } = useBlockTask('tun-mode', async () => {
    await tunMode.upsert(!tunMode.value)
  })

  return (
    <ProxyButton
      {...props}
      loading={isPending}
      onClick={execute}
      isActive={Boolean(tunMode.value)}
    >
      <SettingsEthernet />
      <span>{m.settings_system_proxy_tun_mode_label()}</span>
    </ProxyButton>
  )
}
