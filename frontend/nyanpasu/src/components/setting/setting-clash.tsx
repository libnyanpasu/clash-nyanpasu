import { DialogRef } from "@/components/base";
import { useClash } from "@/hooks/use-clash";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import { invoke_uwp_tool } from "@/services/cmds";
import getSystem from "@/utils/get-system";
import { ArrowForward, Settings, Shuffle } from "@mui/icons-material";
import {
  IconButton,
  MenuItem,
  Select,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ClashCoreViewer } from "./mods/clash-core-viewer";
import { ClashFieldViewer } from "./mods/clash-field-viewer";
import { ClashPortViewer } from "./mods/clash-port-viewer";
import { ControllerViewer } from "./mods/controller-viewer";
import { GuardState } from "./mods/guard-state";
import { SettingItem, SettingList } from "./mods/setting-comp";
import { WebUIViewer } from "./mods/web-ui-viewer";
import MDYSwitch from "../common/mdy-switch";

const isWIN = getSystem() === "windows";

interface Props {
  onError: (err: Error) => void;
}

const SettingClash = ({ onError }: Props) => {
  const { t } = useTranslation();

  const { clash, version, mutateClash, patchClash } = useClash();
  const { verge, mutateVerge, patchVerge } = useVerge();

  const { ipv6, "allow-lan": allowLan, "log-level": logLevel } = clash ?? {};

  const { enable_random_port = false, verge_mixed_port } = verge ?? {};

  const [loading, setLoading] = useState({
    ipv6: false,
    "allow-lan": false,
    "log-level": false,
  });

  const patchClashWithLoading = async (value: Partial<IConfigData>) => {
    try {
      setLoading((prevLoading) => ({
        ...prevLoading,
        ...Object.fromEntries(Object.keys(value).map((key) => [key, true])),
      }));

      await patchClash(value);
    } finally {
      setLoading((prevLoading) => ({
        ...prevLoading,
        ...Object.fromEntries(Object.keys(value).map((key) => [key, false])),
      }));
    }
  };

  const webRef = useRef<DialogRef>(null);
  const fieldRef = useRef<DialogRef>(null);
  const portRef = useRef<DialogRef>(null);
  const ctrlRef = useRef<DialogRef>(null);
  const coreRef = useRef<DialogRef>(null);

  const onSwitchFormat = (_e: any, value: boolean) => value;

  const onChangeVerge = (patch: Partial<IVergeConfig>) => {
    mutateVerge({ ...verge, ...patch }, false);
  };

  return (
    <SettingList title={t("Clash Setting")}>
      <WebUIViewer ref={webRef} />
      <ClashFieldViewer ref={fieldRef} />
      <ClashPortViewer ref={portRef} />
      <ControllerViewer ref={ctrlRef} />
      <ClashCoreViewer ref={coreRef} />

      <SettingItem label={t("Allow Lan")}>
        <GuardState
          value={allowLan ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onGuard={(e) => patchClashWithLoading({ "allow-lan": e })}
          loading={loading["allow-lan"]}
        >
          <MDYSwitch edge="end" />
        </GuardState>
      </SettingItem>

      <SettingItem label={t("IPv6")}>
        <GuardState
          value={ipv6 ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onGuard={(e) => patchClashWithLoading({ ipv6: e })}
          loading={loading["ipv6"]}
        >
          <MDYSwitch edge="end" />
        </GuardState>
      </SettingItem>

      <SettingItem label={t("Log Level")}>
        <GuardState
          // clash premium 2022.08.26 值为warn
          value={logLevel === "warn" ? "warning" : logLevel ?? "info"}
          onCatch={onError}
          onFormat={(e: any) => e.target.value}
          onGuard={(e) => patchClashWithLoading({ "log-level": e })}
          loading={loading["log-level"]}
        >
          <Select size="small" sx={{ width: 100, "> div": { py: "7.5px" } }}>
            <MenuItem value="debug">Debug</MenuItem>
            <MenuItem value="info">Info</MenuItem>
            <MenuItem value="warning">Warn</MenuItem>
            <MenuItem value="error">Error</MenuItem>
            <MenuItem value="silent">Silent</MenuItem>
          </Select>
        </GuardState>
      </SettingItem>

      <SettingItem
        label={t("Mixed Port")}
        extra={
          <Tooltip title={t("Random Port")}>
            <IconButton
              color={enable_random_port ? "success" : "inherit"}
              size="medium"
              onClick={() => {
                useNotification({
                  title: `${t("Random Port")}: ${
                    enable_random_port ? t("Disable") : t("Enable")
                  }`,
                  body: t("After restart to take effect"),
                });
                onChangeVerge({ enable_random_port: !enable_random_port });
                patchVerge({ enable_random_port: !enable_random_port });
              }}
            >
              <Shuffle
                fontSize="inherit"
                style={{ cursor: "pointer", opacity: 0.75 }}
              />
            </IconButton>
          </Tooltip>
        }
      >
        <TextField
          disabled={enable_random_port}
          autoComplete="off"
          size="small"
          value={verge_mixed_port ?? 7890}
          sx={{ width: 100, input: { py: "7.5px", cursor: "pointer" } }}
          onClick={(e) => {
            portRef.current?.open();
            (e.target as any).blur();
          }}
        />
      </SettingItem>

      <SettingItem label={t("External")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={() => ctrlRef.current?.open()}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem label={t("Web UI")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={() => webRef.current?.open()}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem label={t("Clash Field")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={() => fieldRef.current?.open()}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem
        label={t("Clash Core")}
        extra={
          <IconButton
            color="inherit"
            size="small"
            onClick={() => coreRef.current?.open()}
          >
            <Settings
              fontSize="inherit"
              style={{ cursor: "pointer", opacity: 0.75 }}
            />
          </IconButton>
        }
      >
        <Typography sx={{ py: "7px", pr: 1 }}>{version}</Typography>
      </SettingItem>
      {isWIN && (
        <SettingItem label={t("Open UWP tool")}>
          <IconButton
            color="inherit"
            size="small"
            sx={{ my: "2px" }}
            onClick={invoke_uwp_tool}
          >
            <ArrowForward />
          </IconButton>
        </SettingItem>
      )}
    </SettingList>
  );
};

export default SettingClash;
