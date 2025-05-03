import { useTranslation } from 'react-i18next'
import DataPanel from '@/components/dashboard/data-panel'
import HealthPanel from '@/components/dashboard/health-panel'
import ProxyShortcuts from '@/components/dashboard/proxy-shortcuts'
import ServiceShortcuts from '@/components/dashboard/service-shortcuts'
import Grid from '@mui/material/Grid2'
import { BasePage, cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { Button } from '@libnyanpasu/material-design-react'

export const Route = createFileRoute('/dashboard')({
  component: Dashboard,
})

function Dashboard() {
  const { t } = useTranslation()

  return (
    <BasePage title={t('Dashboard')}>
      {/* <Grid container spacing={2}>
        <DataPanel />

        <HealthPanel />

        <ProxyShortcuts />

        <ServiceShortcuts />
      </Grid> */}

      <div
        className={cn(
          'grid gap-4',
          'grid-cols-12',
          "bg-inherit-allow-fallback"
        )}
      >
        <DataPanel />

        <HealthPanel />

        
      </div>

      <Button variant='flat'>Click me</Button>
    </BasePage>
  )
}
