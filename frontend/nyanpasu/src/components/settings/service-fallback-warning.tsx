import WarningRounded from '~icons/material-symbols/warning-rounded'
import { useMemo } from 'react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { useCoreStatus, useSetting, useSystemService } from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'

/**
 * Why service mode, although switched on, is not what runs the core. The
 * backend runs the core locally whenever the service is not usable (#5443),
 * so the host the core actually runs on is the fact; the service status only
 * explains it. `null` while that is unknown or service mode is honoured.
 */
export const useServiceFallbackReason = () => {
  const serviceMode = useSetting('enable_service_mode')

  const { data: coreStatus } = useCoreStatus()

  const {
    query: { data: serviceStatus },
  } = useSystemService()

  return useMemo(() => {
    if (!serviceMode.value || !coreStatus || !serviceStatus) {
      return null
    }

    if (coreStatus.type === 'service') {
      return null
    }

    const compat = serviceStatus.compat.kind

    if (serviceStatus.status === 'not_installed') {
      return m.service_mode_fallback_not_installed()
    }

    // Starting an incompatible service would not help, so that comes first.
    if (compat === 'incompatible' || compat === 'unparsable') {
      return m.service_mode_fallback_incompatible()
    }

    if (serviceStatus.status === 'stopped') {
      return m.service_mode_fallback_stopped()
    }

    return m.service_mode_fallback_unavailable()
  }, [serviceMode.value, coreStatus, serviceStatus])
}

export default function ServiceFallbackWarning({
  className,
}: {
  className?: string
}) {
  const reason = useServiceFallbackReason()

  if (!reason) {
    return null
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className={cn(
            'inline-flex shrink-0 cursor-help rounded-full text-amber-600 outline-none dark:text-yellow-400',
            'focus-visible:ring-primary focus-visible:ring-2',
            className,
          )}
          aria-label={reason}
          data-slot="service-fallback-warning"
        >
          <WarningRounded className="size-5" aria-hidden="true" />
        </button>
      </TooltipTrigger>

      <TooltipContent>
        <span>{reason}</span>
      </TooltipContent>
    </Tooltip>
  )
}
