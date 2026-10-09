import NetworkPing from '~icons/material-symbols/network-ping-rounded'
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded'
import { Button, ButtonProps } from '@nyanpasu/ui/button'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { ShapeToggle } from '@nyanpasu/ui/shape-toggle'
import { useSystemProxy, useTunMode } from '@/hooks/use-proxy-settings'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'

const ProxyButton = ({
  className,
  isActive,
  loading,
  disabled,
  children,
  ...props
}: ButtonProps & {
  isActive?: boolean
}) => {
  return (
    <Button
      className={cn(
        'group h-16 rounded-3xl font-bold text-nowrap',
        'flex items-center justify-between gap-2',
        'data-[active=false]:bg-white dark:data-[active=false]:bg-black',
        className,
      )}
      data-active={String(Boolean(isActive))}
      data-loading={String(Boolean(loading))}
      disabled={loading || disabled}
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

export const SystemProxyButton = ({
  presentation,
  compact,
  elastic,
  ...props
}: Omit<ButtonProps, 'children' | 'loading'> & {
  presentation?: 'cookie'
  compact?: boolean
  elastic?: boolean
}) => {
  const { execute, isPending, isActive } = useSystemProxy()
  if (presentation === 'cookie')
    return (
      <ShapeToggle
        compact={compact}
        elastic={elastic}
        {...props}
        active={Boolean(isActive)}
        loading={isPending}
        onClick={execute}
        icon={<NetworkPing />}
        label={m.settings_system_proxy_system_proxy_label()}
      />
    )

  return (
    <ProxyButton
      {...props}
      loading={isPending}
      onClick={execute}
      isActive={isActive}
    >
      <NetworkPing />
      <span>{m.settings_system_proxy_system_proxy_label()}</span>
    </ProxyButton>
  )
}

export const TunModeButton = ({
  presentation,
  compact,
  elastic,
  ...props
}: Omit<ButtonProps, 'children' | 'loading'> & {
  presentation?: 'cookie'
  compact?: boolean
  elastic?: boolean
}) => {
  const { execute, isPending, isActive } = useTunMode()
  if (presentation === 'cookie')
    return (
      <ShapeToggle
        compact={compact}
        elastic={elastic}
        {...props}
        active={Boolean(isActive)}
        loading={isPending}
        onClick={execute}
        icon={<SettingsEthernet />}
        label={m.settings_system_proxy_tun_mode_label()}
      />
    )

  return (
    <ProxyButton
      {...props}
      loading={isPending}
      onClick={execute}
      isActive={isActive}
    >
      <SettingsEthernet />
      <span>{m.settings_system_proxy_tun_mode_label()}</span>
    </ProxyButton>
  )
}
