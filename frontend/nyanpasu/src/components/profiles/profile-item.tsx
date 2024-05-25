import parseTraffic from "@/utils/parse-traffic";
import { Update, FilterDrama, InsertDriveFile } from "@mui/icons-material";
import LoadingButton from "@mui/lab/LoadingButton";
import {
  Paper,
  Button,
  LinearProgress,
  Chip,
  Tooltip,
  Menu,
  MenuItem,
} from "@mui/material";
import { Profile } from "@nyanpasu/interface";
import dayjs from "dayjs";
import { memo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ProfileDialog } from "./profile-dialog";

export interface ProfileItemProps {
  item: Profile.Item;
}

export const ProfileItem = memo(function ProfileItem({
  item,
}: ProfileItemProps) {
  const { t } = useTranslation();

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

  const menuMapping = {
    Edit: () => setOpen(true),
  };

  const [open, setOpen] = useState(false);

  return (
    <>
      <Paper className="p-5 flex flex-col gap-4" sx={{ borderRadius: 6 }}>
        <div className="flex items-center justify-between">
          <Tooltip title={item.url}>
            <Chip
              className="!pl-2 !pr-2 font-bold"
              avatar={<IconComponent className="!size-5" color="primary" />}
              label={isRemote ? "Remote" : "Local"}
            />
          </Tooltip>

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
                {(used / total).toFixed(2)}%
              </div>
            </Tooltip>
          </div>
        )}

        <div className="flex gap-2 justify-end">
          {isRemote && (
            <LoadingButton
              size="small"
              variant="outlined"
              startIcon={<Update />}
            >
              {t("Update")}
            </LoadingButton>
          )}

          <Button
            size="small"
            variant="outlined"
            onClick={(e) => setAnchorEl(e.currentTarget)}
          >
            {t("Menu")}
          </Button>

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
