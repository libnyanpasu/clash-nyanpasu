import { useTranslation } from 'react-i18next'
import ProxiesProvider from '@/components/providers/proxies-provider'
import RulesProvider from '@/components/providers/rules-provider'
import UpdateProviders from '@/components/providers/update-providers'
import UpdateProxiesProviders from '@/components/providers/update-proxies-providers'
import { Chip } from '@mui/material'
import Grid from '@mui/material/Grid'
import {
  useClashProxiesProvider,
  useClashRulesProvider,
} from '@nyanpasu/interface'
import { BasePage } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(legacy)/providers')({
  component: ProvidersPage,
})

function ProvidersPage() {
  const { t } = useTranslation()

  const proxiesProvider = useClashProxiesProvider()

  const rulesProvider = useClashRulesProvider()

  return (
    <BasePage title={t('Providers')}>
      <div className="flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <Chip
            className="!h-10 truncate !rounded-full !p-2 !text-lg font-bold"
            label={`${t(`Proxies Providers`)} (${Object.entries(proxiesProvider.data ?? {}).length})`}
          />

          <UpdateProxiesProviders />
        </div>

        {proxiesProvider.data && (
          <Grid container spacing={2}>
            {Object.entries(proxiesProvider.data).map(([name, provider]) => (
              <Grid
                key={name}
                className="w-full"
                size={{
                  sm: 12,
                  md: 6,
                  lg: 4,
                  xl: 3,
                }}
              >
                <ProxiesProvider provider={provider} />
              </Grid>
            ))}
          </Grid>
        )}

        <div className="flex items-center justify-between">
          <Chip
            className="!h-10 truncate !rounded-full !p-2 !text-lg font-bold"
            label={`${t(`Rules Providers`)} (${Object.entries(rulesProvider.data ?? {}).length})`}
          />

          <UpdateProviders />
        </div>

        {rulesProvider.data && (
          <Grid container spacing={2}>
            {Object.entries(rulesProvider.data).map(([name, provider]) => (
              <Grid
                key={name}
                size={{
                  sm: 12,
                  md: 6,
                  lg: 4,
                  xl: 3,
                }}
              >
                <RulesProvider provider={provider} />
              </Grid>
            ))}
          </Grid>
        )}
      </div>
    </BasePage>
  )
}
