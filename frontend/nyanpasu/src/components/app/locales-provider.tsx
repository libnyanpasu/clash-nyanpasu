import { useNyanpasu } from "@nyanpasu/interface";
import { locale } from "dayjs";
import { changeLanguage } from "i18next";
import { useEffect } from "react";

export const LocalesProvider = () => {
  const { nyanpasuConfig } = useNyanpasu();

  useEffect(() => {
    if (nyanpasuConfig?.language) {
      locale(
        nyanpasuConfig?.language === "zh" ? "zh-cn" : nyanpasuConfig?.language,
      );

      changeLanguage(nyanpasuConfig?.language);
    }
  }, [nyanpasuConfig?.language]);

  return null;
};

export default LocalesProvider;
