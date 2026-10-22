import { ComponentProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { useNavigate } from '@tanstack/react-router'
import { useDeleteProfile } from '../../_modules/use-delete-profile'
import { Route as IndexRoute } from '../$uid'

export default function DeleteProfile({
  profile,
  ...props
}: Omit<ComponentProps<typeof Button>, 'loading' | 'onClick'> & {
  profile: ProfileItem_Serialize
}) {
  const { type } = IndexRoute.useParams()

  const navigate = useNavigate()

  const { handleClick, isPending } = useDeleteProfile(profile, {
    onSuccess: async () => {
      await navigate({
        to: `/main/profiles/$type`,
        params: { type },
      })
    },
  })

  return <Button {...props} onClick={handleClick} loading={isPending} />
}
