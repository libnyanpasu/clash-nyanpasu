import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { Profile } from '@nyanpasu/interface'

export default function ViewContent({
  profile,
  ...props
}: Omit<ComponentProps<typeof Button>, 'loading' | 'onClick'> & {
  profile: Profile
}) {
  const handleClick = useLockFn(async () => {
    // TODO: implement view content
  })

  return <Button {...props} onClick={handleClick} />
}
