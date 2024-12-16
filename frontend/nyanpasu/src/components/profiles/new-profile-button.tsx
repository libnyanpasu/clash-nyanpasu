import { use, useEffect, useState } from 'react'
import { Add } from '@mui/icons-material'
import { cn, FloatingButton } from '@nyanpasu/ui'
import { AddProfileContext, ProfileDialog } from './profile-dialog'

export const NewProfileButton = ({ className }: { className?: string }) => {
  const addProfileCtx = use(AddProfileContext)
  const [open, setOpen] = useState(!!addProfileCtx)
  useEffect(() => {
    setOpen(!!addProfileCtx)
  }, [addProfileCtx])
  return (
    <>
      <FloatingButton className={cn(className)} onClick={() => setOpen(true)}>
        <Add className="absolute !size-8" />
      </FloatingButton>

      <ProfileDialog open={open} onClose={() => setOpen(false)} />
    </>
  )
}

export default NewProfileButton
