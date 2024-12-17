export const languageOptions = {
  zh: '简体中文',
  en: 'English',
  ru: 'Русский',
}

export const languageQuirks: {
  [key: string]: {
    drawer: {
      minWidth: number
      itemClassNames?: string
    }
  }
} = {
  zh: {
    drawer: {
      minWidth: 180,
      itemClassNames: 'text-center',
    },
  },
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
}
