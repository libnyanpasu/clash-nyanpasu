export const languageOptions = {
  zh: "中文",
  en: "English",
  ru: "Русский",
};

export const languageQuirks: {
  [key: string]: {
    drawer: {
      minWidth: number;
      itemClassNames?: string;
    };
  };
} = {
  zh: {
    drawer: {
      minWidth: 22,
      itemClassNames: "text-center",
    },
  },
  en: {
    drawer: {
      minWidth: 26,
    },
  },
  ru: {
    drawer: {
      minWidth: 26,
    },
  },
};
