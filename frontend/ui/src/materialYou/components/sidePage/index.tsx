import { motion } from 'framer-motion'
import { FC, ReactNode, Ref } from 'react'
import { cn } from '@/utils'
import * as ScrollArea from '@radix-ui/react-scroll-area'
import { BaseErrorBoundary } from '../basePage/baseErrorBoundary'
import Header from '../basePage/header'
import style from './style.module.scss'

interface Props {
  title?: ReactNode
  header?: ReactNode
  children?: ReactNode
  sideBar?: ReactNode
  side?: ReactNode
  sideClassName?: string
  portalRightRoot?: ReactNode
  noChildrenScroll?: boolean
  flexReverse?: boolean
  leftViewportRef?: Ref<HTMLDivElement>
  rightViewportRef?: Ref<HTMLDivElement>
}

export const SidePage: FC<Props> = ({
  title,
  header,
  children,
  sideBar,
  side,
  sideClassName,
  portalRightRoot,
  flexReverse,
  leftViewportRef,
  rightViewportRef,
}) => {
  const sideBarStyle = {
    height: sideBar ? 'calc(100% - 56px)' : undefined,
  }

  return (
    <BaseErrorBoundary>
      <div className={style['MDYSidePage-Main']} data-tauri-drag-region>
        <Header title={title} header={header} />

        <div className={style['MDYSidePage-Container']}>
          <div
            className={cn(
              'flex h-full w-full',
              flexReverse && 'flex-row-reverse',
            )}
          >
            <ScrollArea.Root asChild>
              <motion.div
                className="w-1/3"
                initial={false}
                animate={side ? 'open' : 'closed'}
                variants={{
                  open: {
                    opacity: 1,
                    maxWidth: '348px',
                    minWidth: '192px',
                    display: 'block',
                    marginLeft: flexReverse ? '16px' : undefined,
                    marginRight: flexReverse ? undefined : '16px',
                  },
                  closed: {
                    opacity: 0.5,
                    maxWidth: 0,
                    marginLeft: '0px',
                    marginRight: '0px',
                    transitionEnd: {
                      display: 'none',
                    },
                  },
                }}
              >
                {sideBar && <div className="mb-4 h-10">{sideBar}</div>}

                <ScrollArea.Viewport
                  className={cn(
                    style['Container-common'],
                    'relative w-full [&>div]:!block',
                    sideClassName,
                  )}
                  style={sideBarStyle}
                  ref={leftViewportRef}
                >
                  {side}
                </ScrollArea.Viewport>

                <ScrollArea.Scrollbar
                  className={cn(
                    'flex touch-none py-6 pr-1.5 select-none',
                    sideBar && '!top-14',
                  )}
                  orientation="vertical"
                  style={sideBarStyle}
                >
                  <ScrollArea.Thumb
                    className={cn(
                      style['ScrollArea-Thumb'],
                      'relative flex !w-1.5 flex-1 rounded-full',
                    )}
                  />
                </ScrollArea.Scrollbar>

                <ScrollArea.Corner className="ScrollAreaCorner" />
              </motion.div>
            </ScrollArea.Root>

            <ScrollArea.Root
              className={cn(style['Container-common'], 'w-full')}
            >
              {portalRightRoot}

              <ScrollArea.Viewport
                className={cn('relative h-full w-full [&>div]:!block')}
                ref={rightViewportRef}
              >
                {children}
              </ScrollArea.Viewport>

              <ScrollArea.Scrollbar
                className="flex touch-none py-6 pr-1.5 select-none"
                orientation="vertical"
              >
                <ScrollArea.Thumb className="!bg-scroller relative flex !w-1.5 flex-1 rounded-full" />
              </ScrollArea.Scrollbar>

              <ScrollArea.Corner className="ScrollAreaCorner" />
            </ScrollArea.Root>
          </div>
        </div>
      </div>
    </BaseErrorBoundary>
  )
}
