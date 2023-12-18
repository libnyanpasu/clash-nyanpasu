import { BaseDialog, DialogRef } from "@/components/base";
import { useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import { List, ListItem, ListItemText, MenuItem, Select } from "@mui/material";
import { useLockFn } from "ahooks";
import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
export const TasksViewer = forwardRef<DialogRef>(
  function TasksViewer(props, ref) {
    const { t } = useTranslation();
    const { verge, patchVerge } = useVerge();

    const [open, setOpen] = useState(false);
    const [loading, setLoading] = useState(false);
    const [values, setValues] = useState({
      auto_log_clean: 0,
    });

    useImperativeHandle(ref, () => ({
      open: () => {
        setOpen(true);
        setValues({
          auto_log_clean: verge?.auto_log_clean ?? 0,
        });
      },
      close: () => setOpen(false),
    }));
    const onSave = useLockFn(async () => {
      setLoading(true);
      try {
        await patchVerge({
          auto_log_clean: values.auto_log_clean,
        });
        setOpen(false);
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
      } catch (err: any) {
        useNotification(t("Error"), err.message || err.toString());
      } finally {
        setLoading(false);
      }
    });

    return (
      <BaseDialog
        title={t("Tasks")}
        open={open}
        contentSx={{ width: 450 }}
        okBtn={t("Save")}
        cancelBtn={t("Cancel")}
        loading={loading}
        onClose={() => setOpen(false)}
        onCancel={() => setOpen(false)}
        onOk={onSave}
      >
        <List>
          <ListItem sx={{ padding: "5px 2px" }}>
            <ListItemText primary={t("Auto Log Clean")} />
            <Select
              size="small"
              sx={{ width: 135, "> div": { py: "7.5px" } }}
              value={values.auto_log_clean}
              onChange={(e) => {
                setValues((v) => ({
                  ...v,
                  auto_log_clean: e.target.value as number,
                }));
              }}
            >
              {[
                { key: "Never Clean", value: 0 },
                { key: "Retain 3 Days", value: 3 * 24 * 60 },
                { key: "Retain 7 Days", value: 7 * 24 * 60 },
                { key: "Retain 30 Days", value: 30 * 24 * 60 },
                { key: "Retain 90 Days", value: 90 * 24 * 60 },
              ].map((i) => (
                <MenuItem key={i.value} value={i.value}>
                  {t(i.key)}
                </MenuItem>
              ))}
            </Select>
          </ListItem>
        </List>
      </BaseDialog>
    );
  },
);
