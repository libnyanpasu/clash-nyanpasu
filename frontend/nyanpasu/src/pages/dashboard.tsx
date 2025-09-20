import { useTranslation } from 'react-i18next'
import DataPanel from '@/components/dashboard/data-panel'
import HealthPanel from '@/components/dashboard/health-panel'
import ProxyShortcuts from '@/components/dashboard/proxy-shortcuts'
import ServiceShortcuts from '@/components/dashboard/service-shortcuts'
import { useVisibility } from '@/hooks/use-visibility'
import Grid from '@mui/material/Grid'
import { useClashWSContext } from '@nyanpasu/interface'
import { BasePage } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/dashboard')({
  component: Dashboard,
})

function Dashboard() {
  const { t } = useTranslation()
  const visible = useVisibility()
  const { setRecordTraffic } = useClashWSContext()

  // When the page is not visible, reduce the traffic data update frequency
  // to prevent performance issues when the page is restored
  setRecordTraffic(visible)

  return (
    <BasePage title={t('Dashboard')}>
      <Grid container spacing={2}>
        <DataPanel visible={visible} />

        <HealthPanel />

        <ProxyShortcuts />

        <ServiceShortcuts />
      </Grid>
    </BasePage>
  )
}
