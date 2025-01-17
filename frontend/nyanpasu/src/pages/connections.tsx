import { useThrottle } from 'ahooks'
import { lazy, Suspense, useDeferredValue, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { SearchTermCtx } from '@/components/connections/connection-search-term'
import HeaderSearch from '@/components/connections/header-search'
import { FilterAlt } from '@mui/icons-material'
import { IconButton } from '@mui/material'
import { BasePage } from '@nyanpasu/ui'
import { createFileRoute, useBlocker } from '@tanstack/react-router'

const Component = lazy(() => import('@/components/connections/connection-page'))

const ColumnFilterDialog = lazy(
  () => import('@/components/connections/connections-column-filter'),
)

const ConnectionTotal = lazy(
  () => import('@/components/connections/connections-total'),
)

export const Route = createFileRoute('/connections')({
  component: Connections,
})

function Connections() {
  const { t } = useTranslation()

  const [openColumnFilter, setOpenColumnFilter] = useState(false)

  const [searchTerm, setSearchTerm] = useState<string>()
  const throttledSearchTerm = useThrottle(searchTerm, { wait: 150 })

  const [mountTable, setMountTable] = useState(true)
  const deferredMountTable = useDeferredValue(mountTable)
  const { proceed } = useBlocker({
    shouldBlockFn: (args) => {
      setMountTable(false)
      return !mountTable
    },
    withResolver: true,
  })

  useEffect(() => {
    if (!deferredMountTable) {
      proceed?.()
    }
  }, [proceed, deferredMountTable])

  return (
    <SearchTermCtx.Provider value={throttledSearchTerm}>
      <BasePage
        title={t('Connections')}
        full
        header={
          <div className="flex max-h-96 w-full flex-1 items-center justify-between gap-2 pl-5">
            <ConnectionTotal />
            <div className="flex items-center gap-1">
              <Suspense fallback={null}>
                <ColumnFilterDialog
                  open={openColumnFilter}
                  onClose={() => setOpenColumnFilter(false)}
                />
              </Suspense>
              <HeaderSearch
                value={searchTerm}
                onChange={(e) => setSearchTerm(e.target.value)}
              />
              <IconButton onClick={() => setOpenColumnFilter(true)}>
                <FilterAlt />
              </IconButton>
            </div>
          </div>
        }
      >
        {mountTable && <Component />}
      </BasePage>
    </SearchTermCtx.Provider>
  )
}
