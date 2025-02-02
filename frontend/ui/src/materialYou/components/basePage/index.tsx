import { motion } from 'framer-motion'
import { CSSProperties, FC, ReactNode, Ref, Suspense } from 'react'
import { cn } from '@/utils'
import * as ScrollArea from '@radix-ui/react-scroll-area'
import { BaseErrorBoundary } from './baseErrorBoundary'
import Header from './header'
import './style.scss'

interface BasePageProps {
  title?: ReactNode
  header?: ReactNode
  contentStyle?: CSSProperties
  sectionStyle?: CSSProperties
  full?: boolean
  viewportRef?: Ref<HTMLDivElement>
  children?: ReactNode
}

export const BasePage: FC<BasePageProps> = ({
  title,
  header,
  contentStyle,
  sectionStyle,
  full,
  viewportRef,
  children,
}) => {
  return (
    <BaseErrorBoundary>
      <div className="MDYBasePage" data-tauri-drag-region>
        <Header title={title} header={header} />

        <ScrollArea.Root
          className="MDYBasePage-container relative h-full w-full overflow-hidden rounded-3xl"
          style={contentStyle}
        >
          <ScrollArea.Viewport
            className={cn(
              'relative h-full w-full [&>div]:!block',
              full ?? 'p-6',
            )}
            ref={viewportRef}
            style={sectionStyle}
          >
            <Suspense>
              <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }}>
                {children}
              </motion.div>
            </Suspense>
          </ScrollArea.Viewport>

          <ScrollArea.Scrollbar
            className="flex touch-none py-6 pr-1.5 select-none"
            orientation="vertical"
          >
            <ScrollArea.Thumb className="ScrollArea-Thumb relative flex !w-1.5 flex-1 rounded-full" />
          </ScrollArea.Scrollbar>

          {/* <ScrollArea.Scrollbar
            className="ScrollAreaScrollbar"
            orientation="horizontal"
          >
            <ScrollArea.Thumb className="ScrollAreaThumb" />
          </ScrollArea.Scrollbar> */}
          <ScrollArea.Corner className="ScrollAreaCorner" />
        </ScrollArea.Root>
      </div>
    </BaseErrorBoundary>
  )
}

export const ScrollAreaViewport = ScrollArea.Viewport
