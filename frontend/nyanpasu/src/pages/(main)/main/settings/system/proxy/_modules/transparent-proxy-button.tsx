import ShieldRounded from '~icons/material-symbols/shield-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalDescription,
  ModalTitle,
} from '@nyanpasu/ui/modal'
import { ProxyButton } from '@/components/settings/system-proxy'
import { useTransparentProxy } from '@/hooks/use-proxy-settings'
import { m } from '@/paraglide/messages'
import { Link } from '@tanstack/react-router'

export default function TransparentProxyButton() {
  const proxy = useTransparentProxy()
  const [open, setOpen] = useState(false)
  if (!proxy.visible) return null

  const handleClick = () => {
    if (proxy.disabled) setOpen(true)
    else proxy.execute()
  }

  return (
    <div className="col-span-2">
      <ProxyButton
        className="w-full"
        isActive={proxy.isActive}
        loading={proxy.isPending}
        disabled={proxy.disabled && !proxy.reason}
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

      <Modal open={open} onOpenChange={setOpen}>
        <ModalContent>
          <Card className="w-96 max-w-full">
            <CardHeader>
              <ModalTitle>
                {m.settings_clash_transparent_proxy_label()}
              </ModalTitle>
            </CardHeader>
            <CardContent>
              <ModalDescription className="text-on-surface-variant text-sm">
                {proxy.reason}
              </ModalDescription>
            </CardContent>
            <CardFooter className="gap-1">
              <ModalClose asChild>
                <Button>{m.common_cancel()}</Button>
              </ModalClose>
              <Button variant="flat" asChild>
                <Link
                  className="flex items-center justify-center"
                  to={
                    proxy.serviceModeEnabled
                      ? '/main/settings/clash/network'
                      : '/main/settings/system/service'
                  }
                  onClick={() => setOpen(false)}
                >
                  {proxy.serviceModeEnabled
                    ? m.settings_label_network()
                    : m.settings_label_service()}
                </Link>
              </Button>
            </CardFooter>
          </Card>
        </ModalContent>
      </Modal>
    </div>
  )
}
