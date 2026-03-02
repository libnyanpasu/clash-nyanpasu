import { Card, CardContent } from '@/components/ui/card'
import { m } from '@/paraglide/messages'
import { useSystemProxy } from '@nyanpasu/interface'

export default function CurrentSystemProxy() {
  const { data } = useSystemProxy()

  return (
    <div
      data-slot="current-system-proxy-container"
      className="flex flex-col gap-0.5 select-text"
    >
      {Object.entries(data ?? []).map(([key, value], index) => {
        return (
          <div key={index} className="flex w-full leading-8">
            <div className="w-28 capitalize">{key}:</div>

            <div className="text-warp flex-1 break-all">{String(value)}</div>
          </div>
        )
      })}
    </div>
  )
}
