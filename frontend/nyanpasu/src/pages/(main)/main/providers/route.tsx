import { motion } from 'framer-motion'
import { ComponentProps } from 'react'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { Button } from '@/components/ui/button'
import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'
import { Sidebar, SidebarContent } from '@/components/ui/sidebar'
import TextMarquee from '@/components/ui/text-marquee'
import useIsMobile from '@/hooks/use-is-moblie'
import {
  useClashProxiesProvider,
  useClashRulesProvider,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { createFileRoute, Link, useLocation } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/providers')({
  component: RouteComponent,
})

const NavigateButton = ({
  className,
  ...props
}: ComponentProps<typeof Button>) => {
  return (
    <Button
      className={cn(
        'h-16 w-full rounded-2xl',
        'flex flex-col items-start justify-center gap-2',
        'data-[active=true]:bg-surface-variant/80',
        'data-[active=false]:bg-transparent',
        'data-[active=false]:shadow-none',
        'data-[active=false]:hover:shadow-none',
        'data-[active=false]:hover:bg-surface-variant/30',
        className,
      )}
      asChild
      {...props}
    />
  )
}

const SidebarNavigate = () => {
  const proxiesProvider = useClashProxiesProvider()

  const proxies = proxiesProvider.data
    ? Object.entries(proxiesProvider.data)
    : null

  const rulesProvider = useClashRulesProvider()

  const rules = rulesProvider.data ? Object.entries(rulesProvider.data) : null

  const { pathname } = useLocation()

  return (
    <>
      {proxies && proxies.length ? (
        <>
          <div className="flex flex-col gap-2 p-2">
            {proxies.map(([key, data]) => (
              <NavigateButton
                key={key}
                data-active={String(pathname.endsWith(`/proxies/${key}`))}
              >
                <Link
                  to="/main/providers/proxies/$key"
                  params={{
                    key,
                  }}
                >
                  <div className="text-sm font-medium">{data.name}</div>

                  <TextMarquee className="text-xs text-zinc-500">
                    {data.type}
                  </TextMarquee>
                </Link>
              </NavigateButton>
            ))}
          </div>

          <Separator />
        </>
      ) : null}

      {rules && rules.length ? (
        <div className="flex flex-col gap-2 p-2">
          {rules.map(([key, data]) => (
            <NavigateButton
              key={key}
              data-active={String(pathname.endsWith(`/rules/${key}`))}
            >
              <Link
                to="/main/providers/rules/$key"
                params={{
                  key,
                }}
              >
                <div className="text-sm font-medium">{data.name}</div>

                <TextMarquee className="text-xs text-zinc-500">
                  {data.type}
                </TextMarquee>
              </Link>
            </NavigateButton>
          ))}
        </div>
      ) : null}
    </>
  )
}

function RouteComponent() {
  const { pathname } = useLocation()

  const isCurrent = pathname === Route.fullPath

  const isMobile = useIsMobile()

  return (
    <Sidebar data-slot="providers-container">
      {!isCurrent && !isMobile && (
        <motion.div
          animate={{
            opacity: 1,
            x: 0,
          }}
          initial={{
            opacity: 0,
            x: -24,
          }}
          transition={{
            duration: 0.28,
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          <SidebarContent
            className="bg-surface-variant/10 [&>div>div]:block!"
            data-slot="providers-sidebar-scroll-area"
          >
            <SidebarNavigate />
          </SidebarContent>
        </motion.div>
      )}

      <AppContentScrollArea
        className={cn(
          'group/providers-content flex-[3_1_auto]',
          'overflow-clip',
        )}
        data-slot="providers-content-scroll-area"
      >
        <div
          className={cn(
            'container mx-auto w-full max-w-7xl',
            'min-h-[calc(100vh-40px-64px)]',
            'sm:min-h-[calc(100vh-40px-48px)]',
          )}
          data-slot="providers-content"
        >
          <AnimatedOutletPreset />
        </div>
      </AppContentScrollArea>
    </Sidebar>
  )
}
