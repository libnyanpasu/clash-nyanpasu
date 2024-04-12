import { getProviders } from "@/services/api";
import { updateProxyProvider } from "@/services/cmds";
import { RefreshRounded } from "@mui/icons-material";
import {
  Button,
  IconButton,
  List,
  ListItem,
  ListItemText,
} from "@mui/material";
import { useLockFn } from "ahooks";
import dayjs from "dayjs";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import useSWR, { mutate } from "swr";
import { BaseDialog } from "../base";

export const ProviderButton = () => {
  const { t } = useTranslation();
  const { data } = useSWR("getProviders", getProviders);

  const [open, setOpen] = useState(false);

  const hasProvider = Object.keys(data || {}).length > 0;

  const handleUpdate = useLockFn(async (key: string) => {
    await updateProxyProvider(key);
    await mutate("getProxies");
    await mutate("getProviders");
  });

  if (!hasProvider) return null;

  return (
    <>
      <Button
        size="small"
        variant="outlined"
        sx={{ textTransform: "capitalize" }}
        onClick={() => setOpen(true)}
      >
        {t("Provider")}
      </Button>

      <BaseDialog
        open={open}
        title={t("Proxy Provider")}
        contentSx={{ width: 400 }}
        disableOk
        cancelBtn={t("Cancel")}
        onClose={() => setOpen(false)}
        onCancel={() => setOpen(false)}
      >
        <List sx={{ py: 0, minHeight: 250 }}>
          {Object.entries(data || {}).map(([key, item]) => {
            const time = dayjs(item.updatedAt);
            return (
              <ListItem sx={{ p: 0 }} key={key}>
                <ListItemText
                  primary={key}
                  secondary={
                    <>
                      <span style={{ marginRight: "4em" }}>
                        Type: {item.vehicleType}
                      </span>
                      <span title={time.format("YYYY-MM-DD HH:mm:ss")}>
                        Updated: {time.fromNow()}
                      </span>
                    </>
                  }
                />
                <IconButton
                  size="small"
                  color="inherit"
                  title="Update Provider"
                  onClick={() => handleUpdate(key)}
                >
                  <RefreshRounded />
                </IconButton>
              </ListItem>
            );
          })}
        </List>
      </BaseDialog>
    </>
  );
};
