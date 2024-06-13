import { GitHub } from "@mui/icons-material";
import { IconButton } from "@mui/material";
import Masonry from "@mui/lab/Masonry";
import { useLockFn } from "ahooks";
import { useTranslation } from "react-i18next";
import { BasePage } from "@nyanpasu/ui";
import { open } from "@tauri-apps/api/shell";
import { motion } from "framer-motion";
import { lazy, Suspense } from "react";

const asyncComponents = [
  () => import("@/components/setting/setting-system-proxy"),
  () => import("@/components/setting/setting-nyanpasu-ui"),
  () => import("@/components/setting/setting-clash-base"),
  () => import("@/components/setting/setting-clash-port"),
  () => import("@/components/setting/setting-clash-external"),
  () => import("@/components/setting/setting-clash-web"),
  () => import("@/components/setting/setting-clash-field"),
  () => import("@/components/setting/setting-clash-core"),
  () => import("@/components/setting/setting-system-behavior"),
  () => import("@/components/setting/setting-system-service"),
  () => import("@/components/setting/setting-nyanpasu-tasks"),
  () => import("@/components/setting/setting-nyanpasu-misc"),
  () => import("@/components/setting/setting-nyanpasu-path"),
  () => import("@/components/setting/setting-nyanpasu-version"),
];

const GithubIcon = () => {
  const toGithubRepo = useLockFn(() => {
    return open("https://github.com/LibNyanpasu/clash-nyanpasu");
  });

  return (
    <IconButton
      color="inherit"
      title="@keiko233/clash-nyanpasu"
      onClick={toGithubRepo}
    >
      <GitHub fontSize="inherit" />
    </IconButton>
  );
};

export default function SettingPage() {
  const { t } = useTranslation();

  return (
    <BasePage title={t("Settings")} header={<GithubIcon />}>
      <Masonry
        columns={{ xs: 1, sm: 1, md: 2 }}
        spacing={3}
        sx={{ width: "calc(100% + 24px)" }}
        sequential
      >
        {asyncComponents.map((item, index) => {
          const AsyncComponent = lazy(item);

          return (
            <motion.div
              key={index}
              initial={{ opacity: 0, y: 64 }}
              animate={{
                opacity: 1,
                y: 0,
                transition: {
                  delay: index * 0.1 + 0.3,
                  type: "spring",
                },
              }}
            >
              <Suspense>
                <AsyncComponent />
              </Suspense>
            </motion.div>
          );
        })}
      </Masonry>
    </BasePage>
  );
}
