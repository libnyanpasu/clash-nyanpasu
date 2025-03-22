import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { message } from '@/utils/notification'
import { Refresh } from '@mui/icons-material'
import LoadingButton from '@mui/lab/LoadingButton'
import { useClashRulesProvider } from '@nyanpasu/interface'

export const UpdateProviders = () => {
  const { t } = useTranslation()

  const [loading, setLoading] = useState(false)

  const rulesProvider = useClashRulesProvider()

  const handleProviderUpdate = useLockFn(async () => {
    if (!rulesProvider.data) {
      message(`No Providers.`, {
        kind: 'info',
        title: t('Info'),
      })

      return
    }

    try {
      setLoading(true)

      await Promise.all(
        Object.entries(rulesProvider.data).map(([name, provider]) => {
          return provider.mutate()
        }),
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
    <LoadingButton
      variant="contained"
      loading={loading}
      startIcon={<Refresh />}
      onClick={handleProviderUpdate}
    >
      {t('Update All Rules Providers')}
    </LoadingButton>
  )
}

export default UpdateProviders
