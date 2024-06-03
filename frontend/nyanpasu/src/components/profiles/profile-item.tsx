import parseTraffic from "@/utils/parse-traffic";
import {
  Update,
  FilterDrama,
  InsertDriveFile,
  FiberManualRecord,
  Terminal,
} from "@mui/icons-material";
import LoadingButton from "@mui/lab/LoadingButton";
import {
  Paper,
  LinearProgress,
  Chip,
  Tooltip,
  Menu,
  MenuItem,
  useTheme,
  Button,
  alpha,
} from "@mui/material";
import { Profile, useClash } from "@nyanpasu/interface";
import dayjs from "dayjs";
import { memo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ProfileDialog } from "./profile-dialog";
import { useMessage } from "@/hooks/use-notification";
import { useLockFn, useSetState } from "ahooks";

export interface ProfileItemProps {
  item: Profile.Item;
  selected?: boolean;
  onClickChains: (item: Profile.Item) => void;
  chainsSelected?: boolean;
}

export const ProfileItem = memo(function ProfileItem({
  item,
  selected,
  onClickChains,
  chainsSelected,
}: ProfileItemProps) {
  const { t } = useTranslation();

  const { palette } = useTheme();

  const {
    setProfilesConfig,
    deleteConnections,
    updateProfile,
    deleteProfile,
    viewProfile,
  } = useClash();

  const [loading, setLoading] = useSetState({
    update: false,
    menu: false,
  });

  const calc = () => {
    let progress = 0;
    let total = 0;
    let used = 0;

    if (item.extra) {
      const { download, upload, total: t } = item.extra;

      total = t;

      used = download + upload;

      progress = (used / total) * 100;
    }

    return { progress, total, used };
  };

  const { progress, total, used } = calc();

  const isRemote = item.type === "remote";

  const IconComponent = isRemote ? FilterDrama : InsertDriveFile;

  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null);

  const handleClick = (func: () => void) => {
    setAnchorEl(null);
    func();
  };

  const handleSelect = useLockFn(async () => {
    if (selected) {
      return;
    }

    try {
      setLoading({ menu: true });

      await setProfilesConfig({ current: item.uid });

      await deleteConnections();
    } catch (err) {
      useMessage(`Error setting profile: \n ${JSON.stringify(err)}`, {
        title: t("Error"),
        type: "error",
      });
    } finally {
      setLoading({ menu: false });
    }
  });

  const handleUpdate = useLockFn(async (proxy?: boolean) => {
    const options: Profile.Option = item.option || {
      with_proxy: false,
      self_proxy: false,
    };

    if (proxy) {
      if (item.option?.self_proxy) {
        options.with_proxy = false;
        options.self_proxy = true;
      } else {
        options.with_proxy = true;
        options.self_proxy = false;
      }
    }

    try {
      setLoading({ update: true });

      await updateProfile(item.uid, options);
    } finally {
      setLoading({ update: false });
    }
  });

  const handleDelete = useLockFn(async () => {
    try {
      await deleteProfile(item.uid);
    } catch (err) {
      useMessage(`Delete failed: \n ${JSON.stringify(err)}`, {
        title: t("Error"),
        type: "error",
      });
    }
  });

  const menuMapping = {
    Select: () => handleSelect(),
    Edit: () => setOpen(true),
    Chains: () => onClickChains(item),
    "Open File": () => viewProfile(item.uid),
    Update: () => handleUpdate(),
    "Update(Proxy)": () => handleUpdate(true),
    Delete: () => handleDelete(),
  };

  const [open, setOpen] = useState(false);

  return (
    <>
      <Paper
        className="p-5 flex flex-col gap-4"
        sx={{
          borderRadius: 6,
          backgroundColor: selected
            ? alpha(palette.primary.main, 0.2)
            : undefined,
        }}
      >
        <div className="flex items-center justify-between gap-2">
          <Tooltip title={item.url}>
            <Chip
              className="!pl-2 !pr-2 font-bold"
              avatar={<IconComponent className="!size-5" color="primary" />}
              label={isRemote ? "Remote" : "Local"}
            />
          </Tooltip>

          {selected && (
            <FiberManualRecord
              className="!size-3 mr-auto animate-bounce top-0"
              sx={{ fill: palette.success.main }}
            />
          )}

          <div className="text-sm">
            {item.updated! > 0 ? dayjs(item.updated! * 1000).fromNow() : ""}
          </div>
        </div>

        <div>
          <p className="text-lg font-bold truncate">{item.name}</p>
          <p className="truncate">{item.desc}</p>
        </div>

        {isRemote && (
          <div className="flex items-center justify-between gap-4">
            <div className="w-full">
              <LinearProgress variant="determinate" value={progress} />
            </div>

            <Tooltip title={`${parseTraffic(used)} / ${parseTraffic(total)}`}>
              <div className="text-sm font-bold">
                {((used / total) * 100).toFixed(2)}%
              </div>
            </Tooltip>
          </div>
        )}

        <div className="flex gap-2 justify-end">
          <Button
            className="!mr-auto"
            size="small"
            variant={chainsSelected ? "contained" : "outlined"}
            startIcon={<Terminal />}
            onClick={() => onClickChains(item)}
          >
            Chains
          </Button>

          {isRemote && (
            <LoadingButton
              size="small"
              variant="outlined"
              startIcon={<Update />}
              onClick={menuMapping.Update}
              loading={loading.update}
            >
              {t("Update")}
            </LoadingButton>
          )}

          <LoadingButton
            size="small"
            variant="outlined"
            onClick={(e) => setAnchorEl(e.currentTarget)}
            loading={loading.menu}
          >
            {t("Menu")}
          </LoadingButton>

          <Menu
            anchorEl={anchorEl}
            open={Boolean(anchorEl)}
            onClose={() => setAnchorEl(null)}
          >
            {Object.entries(menuMapping).map(([key, func], index) => {
              return (
                <MenuItem key={index} onClick={() => handleClick(func)}>
                  {t(key)}
                </MenuItem>
              );
            })}
          </Menu>
        </div>
      </Paper>

      <ProfileDialog
        open={open}
        onClose={() => setOpen(false)}
        profile={item}
      />
    </>
  );
});

export default ProfileItem;
