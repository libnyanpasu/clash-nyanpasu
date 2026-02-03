import { nanoid } from 'nanoid'
import { useMemo } from 'react'
import { useProfile, type Profile } from '@nyanpasu/interface'

type CurrentProfileData = Profile & {
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

      if (item.type === 'remote') {
        readOnly = true
      }

      if (item.type === 'remote' || item.type === 'local') {
        schemaType = 'clash'
      }

      if (item.type === 'merge') {
        schemaType = 'merge'
      }

      if (item.type === 'script') {
        if (item.script_type === 'javascript') {
          language = 'javascript'
          extension = 'js'
        }

        if (item.script_type === 'lua') {
          language = 'lua'
          extension = 'lua'
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
