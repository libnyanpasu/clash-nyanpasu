import ShieldRounded from '~icons/material-symbols/shield-rounded'
import { ProxyButton } from '@/components/settings/system-proxy'
import { useTransparentProxy } from '@/hooks/use-proxy-settings'
import { m } from '@/paraglide/messages'
import { Link } from '@tanstack/react-router'

export default function TransparentProxyButton() {
  const proxy = useTransparentProxy()
  const handleClick = proxy.execute
  if (!proxy.visible) return null

  return (
    <div className="col-span-2 flex flex-col gap-2">
      <ProxyButton
        isActive={proxy.isActive}
        loading={proxy.isPending}
        disabled={proxy.disabled}
        onClick={handleClick}
      >
        <ShieldRounded />
        <span>{m.settings_clash_transparent_proxy_label()}</span>
        <span className="text-sm font-normal">
          {proxy.mode === 'redir'
            ? m.settings_clash_transparent_proxy_mode_redir()
            : m.settings_clash_transparent_proxy_mode_tproxy()}
        </span>
      </ProxyButton>

      {proxy.reason && (
        <p className="text-on-surface-variant text-sm">{proxy.reason}</p>
      )}
      {proxy.status?.supported && (proxy.isActive || proxy.status.active) && (
        <p className="text-on-surface-variant text-sm">
          {proxy.status.error ??
            (proxy.applied
              ? m.settings_clash_transparent_proxy_status_applied()
              : m.settings_clash_transparent_proxy_status_pending())}
        </p>
      )}
      <Link
        className="text-primary w-fit text-sm underline-offset-2 hover:underline"
        to="/main/settings/clash/network"
      >
        {m.settings_label_network()}
      </Link>
    </div>
  )
}
