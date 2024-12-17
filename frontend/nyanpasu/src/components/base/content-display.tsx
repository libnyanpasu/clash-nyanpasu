import { ReactNode } from 'react'
import { Public } from '@mui/icons-material'
import { cn } from '@nyanpasu/ui'

export interface ContentDisplayProps {
  className?: string
  message?: string
  children?: ReactNode
}

export const ContentDisplay = ({
  message,
  children,
  className,
}: ContentDisplayProps) => (
  <div
    className={cn('flex h-full w-full items-center justify-center', className)}
  >
    <div className="flex flex-col items-center gap-4">
      {children || (
        <>
          <Public className="!size-16" />

          <b>{message}</b>
        </>
      )}
    </div>
  </div>
)

export default ContentDisplay
