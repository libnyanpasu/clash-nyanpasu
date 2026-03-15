import { useMemo } from 'react'
import Image from '@/components/ui/image'
import { useClashProxies } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'

export default function ProxyIcon({ groupName }: { groupName: string }) {
  const {
    proxies: { data: proxies },
  } = useClashProxies()

  const icon = useMemo(() => {
    const proxyInfo = proxies?.groups.find((p) => p.name === groupName)

    return proxyInfo?.icon
  }, [groupName, proxies])

  return icon ? (
    <Image className="size-6" loadingClassName="rounded-full" icon={icon} />
  ) : (
    <div
      className={cn(
        'bg-surface text-secondary grid size-6 place-content-center rounded-full text-[10px]',
      )}
    >
      {groupName?.toLocaleUpperCase().slice(0, 2)}
    </div>
  )
}
