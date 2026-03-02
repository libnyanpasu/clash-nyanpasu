import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { useScrollArea } from '@/components/ui/scroll-area'
import { cn } from '@nyanpasu/ui'
import { Link } from '@tanstack/react-router'

const TITLE_LAYOUT_ID = 'settings-title-shared'

const BackButton = () => {
  return (
    <Button
      icon
      variant="raised"
      className="flex items-center justify-center md:hidden"
      asChild
    >
      <Link to="/main/settings">
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  )
}

export function SettingsTitlePlaceholder({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('h-4', className)}
      data-slot="settings-title-placeholder"
      {...props}
    />
  )
}

const Title = (props: ComponentProps<typeof motion.p>) => {
  return (
    <motion.p
      layout
      layoutId={TITLE_LAYOUT_ID}
      transition={{
        layout: {
          duration: 0.5,
          ease: [0.32, 0.72, 0, 1],
        },
        opacity: {
          duration: 0.16,
        },
      }}
      {...props}
    />
  )
}

export function SettingsTitle({
  className,
  children,
  ...props
}: ComponentProps<'div'>) {
  const { offset } = useScrollArea()

  const showTopTitle = offset.top > 60

  return (
    <>
      <div
        className={cn(
          'group sticky top-0 z-10',
          'backdrop-blur-xl',
          'flex items-center gap-6',
          'h-16 px-6',
          className,
        )}
        data-show-title={showTopTitle}
        data-slot="settings-title"
        {...props}
      >
        <BackButton />

        <AnimatePresence initial={false}>
          {showTopTitle && (
            <Title key="settings-title-top" className="text-xl font-bold">
              {children}
            </Title>
          )}
        </AnimatePresence>
      </div>

      <div
        className="group flex h-24 px-6 pt-10 pb-4"
        data-slot="settings-title"
        data-show-title={!showTopTitle}
      >
        <AnimatePresence initial={false}>
          {!showTopTitle && (
            <Title key="settings-title-main" className="text-3xl font-bold">
              {children}
            </Title>
          )}
        </AnimatePresence>
      </div>
    </>
  )
}
