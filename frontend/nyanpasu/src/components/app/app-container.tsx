import getSystem from '@/utils/get-system'
import { alpha, useTheme } from '@mui/material'
import Paper from '@mui/material/Paper'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import 'allotment/dist/style.css'
import { useAtomValue } from 'jotai'
import { ReactNode } from 'react'
import { atomIsDrawerOnlyIcon } from '@/store'
import { cn } from '@nyanpasu/ui'
import { LayoutControl } from '../layout/layout-control'
import styles from './app-container.module.scss'
import AppDrawer from './app-drawer'
import DrawerContent from './drawer-content'

const appWindow = getCurrentWebviewWindow()

const OS = getSystem()

export const AppContainer = ({
  children,
  isDrawer,
}: {
  children?: ReactNode
  isDrawer?: boolean
}) => {
  const { palette } = useTheme()

  const onlyIcon = useAtomValue(atomIsDrawerOnlyIcon)

  return (
    <Paper
      square
      elevation={0}
      className={styles.layout}
      onPointerDown={(e: any) => {
        if (e.target?.dataset?.windrag) {
          appWindow.startDragging()
        }
      }}
      onContextMenu={(e) => {
        e.preventDefault()
      }}
    >
      {isDrawer && <AppDrawer data-tauri-drag-region />}

      {!isDrawer && (
        <div className={cn(onlyIcon ? 'w-24' : 'w-64')}>
          <DrawerContent data-tauri-drag-region onlyIcon={onlyIcon} />
        </div>
      )}

      <div className={styles.container}>
        {OS === 'windows' && (
          <LayoutControl className="!z-top fixed right-4 top-2" />
        )}

        {OS === 'macos' && (
          <div
            className="z-top fixed left-4 top-3 h-8 w-[4.5rem] rounded-full"
            style={{ backgroundColor: alpha(palette.primary.main, 0.1) }}
          />
        )}

        <div
          className={OS === 'macos' ? 'h-[2.75rem]' : 'h-9'}
          data-tauri-drag-region
        />

        {children}
      </div>
    </Paper>
  )
}

export default AppContainer
