import { useLockFn, useReactive } from "ahooks";
import { motion } from "framer-motion";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMessage } from "@/hooks/use-notification";
import LoadingButton from "@mui/lab/LoadingButton";
import { Box, List, ListItem } from "@mui/material";
import { ClashCore, useClash, useNyanpasu } from "@nyanpasu/interface";
import { BaseCard, ExpandMore } from "@nyanpasu/ui";
import { ClashCoreItem } from "./modules/clash-core";

export const SettingClashCore = () => {
  const { t } = useTranslation();

  const loading = useReactive({
    mask: false,
    restart: false,
    check: false,
  });

  const [expand, setExpand] = useState(false);

  const {
    nyanpasuConfig,
    setClashCore,
    getClashCore,
    restartSidecar,
    getLatestCore,
    updateCore,
  } = useNyanpasu();

  const { getVersion, deleteConnections } = useClash();

  const version = useMemo(() => {
    const data = getVersion.data;

    return data?.premium
      ? `${data.version} Premium`
      : data?.meta
        ? `${data.version} Meta`
        : data?.version || "-";
  }, [getVersion.data, nyanpasuConfig]);

  const changeClashCore = useLockFn(async (core: ClashCore) => {
    try {
      loading.mask = true;

      await deleteConnections();

      await setClashCore(core);

      useMessage(`Successfully switch to ${core}`, {
        type: "info",
        title: t("Success"),
      });
    } catch (e) {
      useMessage(
        `Switching failed, you could see the details in the log. \nError: ${
          e instanceof Error ? e.message : String(e)
        }`,
        {
          type: "error",
          title: t("Error"),
        },
      );
    } finally {
      loading.mask = false;
    }
  });

  const handleRestart = useLockFn(async () => {
    try {
      loading.restart = true;

      await restartSidecar();

      useMessage(t("Successfully restart core"), {
        type: "info",
        title: t("Success"),
      });
    } catch (e) {
      useMessage("Restart failed, please check log.", {
        type: "error",
        title: t("Error"),
      });
    } finally {
      loading.restart = false;
    }
  });

  const handleCheckUpdates = useLockFn(async () => {
    try {
      loading.check = true;

      await getLatestCore.mutate();
    } catch (e) {
      useMessage("Fetch failed, please check your internet connection.", {
        type: "error",
        title: t("Error"),
      });
    } finally {
      loading.check = false;
    }
  });

  const handleUpdateCore = useLockFn(
    async (core: Required<IVergeConfig>["clash_core"]) => {
      try {
        loading.mask = true;

        await updateCore(core);

        useMessage(`Successfully update core ${core}`, {
          type: "info",
          title: t("Success"),
        });
      } catch (e) {
        useMessage(`Update failed.`, {
          type: "error",
          title: t("Error"),
        });
      } finally {
        loading.mask = false;
      }
    },
  );

  const mergeCores = useMemo(() => {
    return getClashCore.data?.map((item) => {
      const latest = getLatestCore.data?.find(
        (i) => i.core == item.core,
      )?.latest;

      return {
        ...item,
        latest,
      };
    });
  }, [getClashCore.data, getLatestCore.data]);

  return (
    <BaseCard
      label={t("Clash Core")}
      loading={loading.mask}
      labelChildren={<span>{version}</span>}
    >
      <List disablePadding>
        {mergeCores?.map((item, index) => {
          const show = expand || item.core == nyanpasuConfig?.clash_core;

          return (
            <motion.div
              key={index}
              initial={false}
              animate={show ? "open" : "closed"}
              variants={{
                open: {
                  height: "auto",
                  opacity: 1,
                  scale: 1,
                },
                closed: {
                  height: 0,
                  opacity: 0,
                  scale: 0.7,
                },
              }}
            >
              <ClashCoreItem
                data={item}
                selected={item.core == nyanpasuConfig?.clash_core}
                onClick={() => changeClashCore(item.core)}
                onUpdate={() => handleUpdateCore(item.core)}
              />
            </motion.div>
          );
        })}

        <ListItem
          sx={{
            pl: 0,
            pr: 0,
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <Box display="flex" gap={1}>
            <LoadingButton
              variant="outlined"
              loading={loading.restart}
              onClick={handleRestart}
            >
              {t("Restart")}
            </LoadingButton>

            <LoadingButton
              loading={loading.check || getLatestCore.isLoading}
              variant="contained"
              onClick={handleCheckUpdates}
            >
              {t("Check Updates")}
            </LoadingButton>
          </Box>

          <ExpandMore expand={expand} onClick={() => setExpand(!expand)} />
        </ListItem>
      </List>
    </BaseCard>
  );
};

export default SettingClashCore;
