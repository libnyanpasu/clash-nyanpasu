import { Button, Divider } from "@mui/material";
import LoadingButton from "@mui/lab/LoadingButton";
import { useTheme } from "@mui/material/styles";
import {
  CSSProperties,
  ReactNode,
  useEffect,
  useLayoutEffect,
  useState,
} from "react";
import { AnimatePresence, motion } from "framer-motion";
import React from "react";
import useDebounceFn from "ahooks/lib/useDebounceFn";
import { useLockFn } from "ahooks";
import { useTranslation } from "react-i18next";
import * as Dialog from "@radix-ui/react-dialog";
import { cn } from "@/utils";
import { useClickPosition } from "@/hooks";

export interface BaseDialogProps {
  title: ReactNode;
  open: boolean;
  close?: string;
  ok?: string;
  disabledOk?: boolean;
  contentStyle?: CSSProperties;
  children?: ReactNode;
  loading?: boolean;
  onOk?: () => void | Promise<void>;
  onClose?: () => void;
  divider?: boolean;
}

export const BaseDialog = ({
  title,
  open,
  close,
  onClose,
  children,
  contentStyle,
  disabledOk,
  loading,
  onOk,
  ok,
  divider,
}: BaseDialogProps) => {
  const { t } = useTranslation();

  const { palette } = useTheme();

  const [mounted, setMounted] = useState(false);

  const [offset, setOffset] = useState({
    x: 0,
    y: 0,
  });

  const [okLoading, setOkLoading] = useState(false);

  const { run: runMounted, cancel: cancelMounted } = useDebounceFn(
    () => setMounted(false),
    { wait: 300 },
  );

  const clickPosition = useClickPosition();

  useLayoutEffect(() => {
    if (open) {
      setOffset({
        x: clickPosition?.x ?? 0,
        y: clickPosition?.y ?? 0,
      });
    }
  }, [open]);

  const handleClose = () => {
    if (onClose) {
      onClose();
      runMounted();
    }
  };

  const handleOk = useLockFn(async () => {
    if (!onOk) return;

    if (onOk.constructor.name === "AsyncFunction") {
      try {
        setOkLoading(true);

        await onOk();
      } finally {
        setOkLoading(false);
      }
    } else {
      onOk();
    }
  });

  useEffect(() => {
    if (open) {
      setMounted(true);
      cancelMounted();
    }
  }, [open]);

  return (
    <Dialog.Root>
      <AnimatePresence>
        {mounted && (
          <Dialog.Portal forceMount>
            <Dialog.Overlay asChild onClick={handleClose}>
              <motion.div
                className="fixed inset-0 z-50 backdrop-brightness-50"
                animate={open ? "open" : "closed"}
                initial={{
                  opacity: 0,
                }}
                variants={{
                  open: {
                    opacity: 1,
                  },
                  closed: {
                    opacity: 0,
                  },
                }}
              />
            </Dialog.Overlay>

            <Dialog.Content forceMount>
              <motion.div
                className={cn(
                  "fixed z-50 rounded-3xl shadow-lg min-w-96",
                  palette.mode === "dark" ? "text-white" : "text-black",
                )}
                style={{
                  backgroundColor: palette.background.paper,
                }}
                animate={open ? "open" : "closed"}
                initial={{
                  opacity: 0,
                  scale: 0,
                  top: "50%",
                  left: "50%",
                  translateX: "-50%",
                  translateY: "-50%",
                  x: offset.x / 2,
                  y: offset.y / 2,
                }}
                variants={{
                  open: {
                    opacity: 1,
                    scale: 1,
                    x: 0,
                    y: 0,
                  },
                  closed: {
                    opacity: 0,
                    scale: 0,
                    x: offset.x / 2,
                    y: offset.y / 2,
                  },
                }}
                transition={{
                  type: "spring",
                  bounce: 0,
                  duration: 0.35,
                }}
              >
                <Dialog.Title className="text-xl m-4">{title}</Dialog.Title>

                {divider && <Divider />}

                <div
                  className="p-4 overflow-x-hidden overflow-y-auto"
                  style={{
                    maxHeight: "calc(100vh - 160px)",
                    ...contentStyle,
                  }}
                >
                  {children}
                </div>

                {divider && <Divider />}

                <div className="flex gap-2 justify-end m-2">
                  {onClose && (
                    <Button variant="outlined" onClick={handleClose}>
                      {close || t("Close")}
                    </Button>
                  )}

                  {onOk && (
                    <LoadingButton
                      disabled={loading || disabledOk}
                      loading={okLoading || loading}
                      variant="contained"
                      onClick={handleOk}
                    >
                      {ok || t("Ok")}
                    </LoadingButton>
                  )}
                </div>
              </motion.div>
            </Dialog.Content>
          </Dialog.Portal>
        )}
      </AnimatePresence>
    </Dialog.Root>
  );
};
