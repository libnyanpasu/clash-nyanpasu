import { useLockFn } from 'ahooks'
import dayjs from 'dayjs'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { message } from '@/utils/notification'
import { Refresh } from '@mui/icons-material'
import LoadingButton from '@mui/lab/LoadingButton'
import { Chip, Paper } from '@mui/material'
import { ProviderItem, useClashCore } from '@nyanpasu/interface'
import ProxiesProviderTraffic from './proxies-provider-traffic'

export interface ProxiesProviderProps {
  provider: ProviderItem
}

export const ProxiesProvider = ({ provider }: ProxiesProviderProps) => {
  const { t } = useTranslation()

  const [loading, setLoading] = useState(false)

  const { updateProxiesProviders } = useClashCore()

  const handleClick = useLockFn(async () => {
    try {
      setLoading(true)

      await updateProxiesProviders(provider.name)
    } catch (e) {
      message(`Update ${provider.name} failed.\n${String(e)}`, {
        kind: 'error',
        title: t('Error'),
      })
    } finally {
      setLoading(false)
    }
  })

  return (
    <Paper
      className="flex h-full flex-col justify-between gap-2 p-5"
      sx={{
        borderRadius: 6,
      }}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="ml-1">
          <p className="truncate text-lg font-bold">{provider.name}</p>

          <p className="truncate text-sm">
            {provider.vehicleType}/{provider.type}
          </p>
        </div>

        <div className="text-right text-sm">
          {t('Last Update', {
            fromNow: dayjs(provider.updatedAt).fromNow(),
          })}
        </div>
      </div>

      {provider.subscriptionInfo && (
        <ProxiesProviderTraffic provider={provider} />
      )}

      <div className="flex items-center justify-between">
        <Chip
          className="truncate font-bold"
          label={t('Proxy Set proxies', {
            rule: provider.proxies.length,
          })}
        />

        <LoadingButton
          loading={loading}
          size="small"
          variant="contained"
          className="!size-8 !min-w-0"
          onClick={handleClick}
        >
          <Refresh />
        </LoadingButton>
      </div>
    </Paper>
  )
}

export default ProxiesProvider
