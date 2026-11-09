import NetworkPing from '~icons/material-symbols/network-ping-rounded'
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded'
import ShieldRounded from '~icons/material-symbols/shield-rounded'
import { Button, ButtonProps } from '@nyanpasu/ui/button'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { ShapeToggle } from '@nyanpasu/ui/shape-toggle'
import {
  useSystemProxy,
  useTransparentProxy,
  useTunMode,
} from '@/hooks/use-proxy-settings'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'

export const ProxyButton = ({
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
        'group h-14 font-semibold text-nowrap',
        'flex items-center justify-between gap-2',
        'data-[active=false]:bg-surface data-[active=false]:text-on-surface',
        'disabled:bg-surface disabled:text-on-surface/40 disabled:shadow-sm',
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
  const { execute, isPending, isActive, disabled } = useTunMode()
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
        disabled={disabled || props.disabled}
        title={
          disabled ? m.error_transparent_proxy_tun_conflict() : props.title
        }
      />
    )

  return (
    <ProxyButton
      {...props}
      loading={isPending}
      onClick={execute}
      isActive={isActive}
      disabled={disabled || props.disabled}
      title={disabled ? m.error_transparent_proxy_tun_conflict() : props.title}
    >
      <SettingsEthernet />
      <span>{m.settings_system_proxy_tun_mode_label()}</span>
    </ProxyButton>
  )
}

export const TransparentProxyButton = ({
  compact,
  elastic,
  ...props
}: Omit<ButtonProps, 'children' | 'loading'> & {
  compact?: boolean
  elastic?: boolean
}) => {
  const proxy = useTransparentProxy()
  const handleClick = proxy.execute
  if (!proxy.visible) return null
  return (
    <ShapeToggle
      compact={compact}
      elastic={elastic}
      {...props}
      active={proxy.isActive}
      loading={proxy.isPending}
      disabled={proxy.disabled || props.disabled}
      title={proxy.reason}
      onClick={handleClick}
      icon={<ShieldRounded />}
      label={m.settings_clash_transparent_proxy_label()}
    />
  )
}
