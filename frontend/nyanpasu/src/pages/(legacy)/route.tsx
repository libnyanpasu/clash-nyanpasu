import AppContainer from '@/components/app/app-container'
import LocalesProvider from '@/components/app/locales-provider'
import MutationProvider from '@/components/layout/mutation-provider'
import NoticeProvider from '@/components/layout/notice-provider'
import PageTransition from '@/components/layout/page-transition'
import SchemeProvider from '@/components/layout/scheme-provider'
import UpdaterDialog from '@/components/updater/updater-dialog-wrapper'
import { UpdaterProvider } from '@/hooks/use-updater'
import { FileRouteTypes } from '@/route-tree.gen'
import { atomIsDrawer, memorizedRoutePathAtom } from '@/store'
import { useSettings } from '@nyanpasu/interface'
import { cn, useBreakpoint } from '@nyanpasu/ui'
import { createFileRoute, useLocation } from '@tanstack/react-router'
import 'dayjs/locale/ru'
import 'dayjs/locale/zh-cn'
import 'dayjs/locale/zh-tw'
import { useAtom, useSetAtom } from 'jotai'
import { PropsWithChildren, useEffect } from 'react'
import { SWRConfig } from 'swr'

export const Route = createFileRoute('/(legacy)')({
  component: Layout,
})

const QueryLoaderProvider = ({ children }: PropsWithChildren) => {
  const {
    query: { isLoading },
  } = useSettings()

  return isLoading ? null : children
}

function Layout() {
  const breakpoint = useBreakpoint()

  const setMemorizedPath = useSetAtom(memorizedRoutePathAtom)

  const pathname = useLocation({
    select: (location) => location.pathname,
  })

  useEffect(() => {
    if (pathname !== '/') {
      setMemorizedPath(pathname as FileRouteTypes['fullPaths'])
    }
  }, [pathname, setMemorizedPath])

  const [isDrawer, setIsDrawer] = useAtom(atomIsDrawer)

  useEffect(() => {
    setIsDrawer(breakpoint === 'sm' || breakpoint === 'xs')
  }, [breakpoint, setIsDrawer])

  return (
    <SWRConfig
      value={{
        errorRetryCount: 5,
        revalidateOnMount: true,
        revalidateOnFocus: true,
        refreshInterval: 5000,
      }}
    >
      <QueryLoaderProvider>
        <LocalesProvider />
        <MutationProvider />
        <NoticeProvider />
        <SchemeProvider />
        <UpdaterDialog />
        <UpdaterProvider />
        <AppContainer isDrawer={isDrawer}>
          <PageTransition
            className={cn('absolute inset-4 top-10', !isDrawer && 'left-0')}
          />
        </AppContainer>
      </QueryLoaderProvider>
    </SWRConfig>
  )
}
