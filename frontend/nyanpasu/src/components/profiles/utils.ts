import { isEqual } from 'lodash-es'
import { Profile } from '@nyanpasu/interface'

export const filterProfiles = (items?: Profile.Item[]) => {
  const getItems = (types: (string | { script: string })[]) => {
    return items?.filter((i) => {
      if (!i) return false

      if (typeof i.type === 'string') {
        return types.includes(i.type)
      }

      if (typeof i.type === 'object' && i.type !== null) {
        return types.some(
          (type) =>
            typeof type === 'object' &&
            (i.type as { script: string }).script === type.script,
        )
      }

      return false
    })
  }

  const profiles = getItems([Profile.Type.Local, Profile.Type.Remote])

  const scripts = getItems([
    Profile.Type.Merge,
    Profile.Type.JavaScript,
    Profile.Type.LuaScript,
  ])

  return {
    profiles,
    scripts,
  }
}

export const getLanguage = (type: Profile.Item['type'], snake?: boolean) => {
  switch (true) {
    case isEqual(type, Profile.Type.JavaScript):
    case isEqual(type, Profile.Type.JavaScript.script): {
      return snake ? 'JavaScript' : 'javascript'
    }

    case isEqual(type, Profile.Type.LuaScript):
    case isEqual(type, Profile.Type.LuaScript.script): {
      return snake ? 'Lua' : 'lua'
    }

    case isEqual(type, Profile.Type.Merge): {
      return snake ? 'YAML' : 'yaml'
    }

    default: {
      return
    }
  }
}
