import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@nyanpasu/utils'

export default function ActionButton({
  className,
  ...props
}: ComponentProps<typeof Button>) {
  return <Button className={cn('h-8 min-w-0 px-3', className)} {...props} />
}
