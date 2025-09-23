import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { message } from '@/utils/notification'
import { Refresh } from '@mui/icons-material'
import { useClashProxiesProvider } from '@nyanpasu/interface'
import { MUIButton as Button } from '@nyanpasu/ui'

export const UpdateProxiesProviders = () => {
  const { t } = useTranslation()

  const [loading, setLoading] = useState(false)

  const proxiesProvider = useClashProxiesProvider()

  const handleProviderUpdate = useLockFn(async () => {
    if (!proxiesProvider.data) {
      message(`No Providers.`, {
        kind: 'info',
        title: t('Info'),
      })

      return
    }

    try {
      setLoading(true)

      await Promise.all(
        Object.entries(proxiesProvider.data).map(([_, provider]) =>
          provider.mutate(),
        ),
      )
    } catch (e) {
      message(`Update all failed.\n${String(e)}`, {
        kind: 'error',
        title: t('Error'),
      })
    } finally {
      setLoading(false)
    }
  })

  return (
    <Button
      variant="contained"
      loading={loading}
      startIcon={<Refresh />}
      onClick={handleProviderUpdate}
    >
      {t('Update All Proxies Providers')}
    </Button>
  )
}

export default UpdateProxiesProviders
