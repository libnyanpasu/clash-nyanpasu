export const languageOptions = {
  en: 'English',
  ru: 'Русский',
  zh: '简体中文',
  tw: '繁體中文',
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
  zh: {
    drawer: {
      minWidth: 180,
      itemClassNames: 'text-center',
    },
  },
  tw: {
    drawer: {
      minWidth: 180,
      itemClassNames: 'text-center',
    },
  },
}
