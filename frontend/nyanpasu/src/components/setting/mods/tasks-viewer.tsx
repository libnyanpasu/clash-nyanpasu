import { BaseDialog, DialogRef } from "@/components/base";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import { List, ListItem, ListItemText, TextField } from "@mui/material";
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
      max_log_files: 0,
    });

    useImperativeHandle(ref, () => ({
      open: () => {
        setOpen(true);
        setValues({
          max_log_files: verge?.max_log_files ?? 7,
        });
      },
      close: () => setOpen(false),
    }));
    const onSave = useLockFn(async () => {
      setLoading(true);
      try {
        await patchVerge({
          max_log_files: values.max_log_files,
        });
        setOpen(false);
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
      } catch (err: any) {
        useNotification({
          title: t("Error"),
          body: err.message || err.toString(),
          type: NotificationType.Error,
        });
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
            <ListItemText primary={t("Max Log Files")} />
            <TextField
              size="small"
              type="number"
              value={values.max_log_files}
              sx={{ width: 100 }}
              onChange={(e) => {
                setValues({
                  ...values,
                  max_log_files: Number.parseInt(e.target.value, 10),
                });
              }}
            />
          </ListItem>
        </List>
      </BaseDialog>
    );
  },
);
