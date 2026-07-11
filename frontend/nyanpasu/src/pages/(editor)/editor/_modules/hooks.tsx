import { nanoid } from 'nanoid'
import { useMemo } from 'react'
import {
  isConfigItem,
  isRemoteItem,
  isTransformItem,
  useProfile,
  type ProfileItem_Serialize,
} from '@nyanpasu/interface'

type CurrentProfileData = ProfileItem_Serialize & {
  language: string
  extension: string
  readOnly: boolean
  virtualPath: string
}

export function useCurrentProfile(uid: string): {
  data: CurrentProfileData | undefined
} & Omit<ReturnType<typeof useProfile>['query'], 'data'> {
  const profiles = useProfile()

  const currentProfile = useMemo(() => {
    const item = profiles.query.data?.items?.find((item) => item.uid === uid)

    if (item) {
      let language = 'yaml'
      let extension = 'yaml'
      let readOnly = false
      let schemaType

      if (isConfigItem(item)) {
        schemaType = 'clash'
        readOnly = isRemoteItem(item)
      } else if (isTransformItem(item)) {
        if (item.transform.type === 'overlay') {
          schemaType = 'merge'
        } else if (item.transform.type === 'script') {
          if (item.transform.runtime === 'javascript') {
            language = 'javascript'
            extension = 'js'
          } else if (item.transform.runtime === 'lua') {
            language = 'lua'
            extension = 'lua'
          }
        }
      }

      return {
        ...item,
        language,
        extension,
        readOnly,
        virtualPath: `${nanoid()}${schemaType ? `.${schemaType}` : ''}.${language}`,
      }
    }
  }, [profiles.query.data, uid])

  return {
    ...profiles.query,
    data: currentProfile,
  }
}
