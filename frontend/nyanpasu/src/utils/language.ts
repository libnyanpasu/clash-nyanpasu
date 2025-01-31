export const languageOptions = {
  en: 'English',
  ru: 'Русский',
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
}

export const languageQuirks: {
  [key: string]: {
    drawer: {
      minWidth: number
      itemClassNames?: string
    }
  }
} = {
  en: {
    drawer: {
      minWidth: 240,
    },
  },
  ru: {
    drawer: {
      minWidth: 240,
    },
  },
  'zh-CN': {
    drawer: {
      minWidth: 180,
      itemClassNames: 'text-center',
    },
  },
  'zh-TW': {
    drawer: {
      minWidth: 180,
    },
  },
}
