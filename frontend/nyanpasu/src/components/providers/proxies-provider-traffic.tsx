import parseTraffic from '@/utils/parse-traffic'
import { LinearProgress, Tooltip } from '@mui/material'
import { ProxiesProviderProps } from './proxies-provider'

export const ProxiesProviderTraffic = ({ provider }: ProxiesProviderProps) => {
  const calc = () => {
    let progress = 0
    let total = 0
    let used = 0

    if (provider.subscriptionInfo) {
      const {
        Download: download,
        Upload: upload,
        Total: t,
      } = provider.subscriptionInfo

      total = t ?? 0

      used = (download ?? 0) + (upload ?? 0)

      progress = (used / (total ?? 0)) * 100
    }

    return { progress, total, used }
  }

  const { progress, total, used } = calc()

  return (
    <div className="flex items-center justify-between gap-4">
      <div className="w-full">
        <LinearProgress variant="determinate" value={progress} />
      </div>

      <Tooltip title={`${parseTraffic(used)} / ${parseTraffic(total)}`}>
        <div className="text-sm font-bold">
          {((used / total) * 100).toFixed(2)}%
        </div>
      </Tooltip>
    </div>
  )
}

export default ProxiesProviderTraffic
