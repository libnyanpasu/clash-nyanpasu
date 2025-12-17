import { ComponentProps } from 'react'
import { Button, ButtonProps } from '@/components/ui/button'
import { cn } from '@nyanpasu/ui'

const MenuButton = ({ className, ...props }: ButtonProps) => {
  return (
    <Button
      className={cn(
        'hover:bg-primary-container dark:hover:bg-on-primary h-8 min-w-0 px-3',
        className,
      )}
      {...props}
    />
  )
}

// TODO: implement menu items
export default function HeaderMenu({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex items-center gap-0.5', className)}
      {...props}
      data-tauri-drag-region
    >
      <MenuButton>Files</MenuButton>
      <MenuButton>Actions</MenuButton>
      <MenuButton>Settings</MenuButton>
      <MenuButton>Help</MenuButton>
    </div>
  )
}
