import { nanoid } from 'nanoid'
import { useMemo, useState } from 'react'
import {
  isConfigItem,
  isRemoteItem,
  isTransformItem,
  useProfile,
} from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'

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

  // The model path must stay stable across profiles refetches: a new path
  // makes @monaco-editor/react create a fresh model from the saved content and
  // swap it in, dropping the user's unsaved edits from the screen.
  const [modelId] = useState(() => nanoid())

  const currentProfile = useMemo(() => {
    const item = profiles.query.data?.items?.find((item) => item.uid === uid)

    if (item) {
      let language = 'yaml'
      let extension = 'yaml'
      // A remote source is overwritten by the next refresh.
      const readOnly = isRemoteItem(item)
      let schemaType

      if (isConfigItem(item)) {
        schemaType = 'clash'
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
        virtualPath: `${modelId}${schemaType ? `.${schemaType}` : ''}.${language}`,
      }
    }
  }, [profiles.query.data, uid, modelId])

  return {
    ...profiles.query,
    data: currentProfile,
  }
}
