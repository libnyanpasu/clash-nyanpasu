import ShieldRounded from '~icons/material-symbols/shield-rounded'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { useTransparentProxy } from '@/hooks/use-proxy-settings'
import { m } from '@/paraglide/messages'
import { ActionButton } from './action-button'

export const TransparentProxyButton = () => {
  const proxy = useTransparentProxy()
  const handleClick = proxy.execute
  if (!proxy.visible) return null
  return (
    <ActionButton
      className="col-span-2 h-14 flex-col items-start gap-0.5"
      checked={proxy.isActive}
      disabled={proxy.disabled || proxy.isPending}
      title={proxy.reason}
      onClick={handleClick}
    >
      <ShieldRounded />
      <TextMarquee className="w-full min-w-0">
        {m.settings_clash_transparent_proxy_label()}:{' '}
        {proxy.isActive ? m.common_enabled() : m.common_disabled()}
      </TextMarquee>
    </ActionButton>
  )
}
