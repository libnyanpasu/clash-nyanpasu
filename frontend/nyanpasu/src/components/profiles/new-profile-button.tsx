import { Add } from "@mui/icons-material";
import { Profile, useClash } from "@nyanpasu/interface";
import { BaseDialog, FloatingButton } from "@nyanpasu/ui";
import { useRef, useState } from "react";
import { useForm, Controller } from "react-hook-form";
import { SelectElement, TextFieldElement } from "react-hook-form-mui";
import { useTranslation } from "react-i18next";
import { version } from "~/package.json";
import { LabelSwitch } from "../setting/modules/clash-field";
import { ReadProfile } from "./read-profile";
import { InputAdornment } from "@mui/material";

export const NewProfileButton = () => {
  const { t } = useTranslation();

  const { createProfile } = useClash();

  const [open, setOpen] = useState(false);

  const localProfile = useRef("");

  const [localProfileMessage, setLocalProfileMessage] = useState("");

  const { control, watch, handleSubmit, reset } = useForm<Profile.Item>({
    defaultValues: {
      type: "remote",
      name: `New Profile`,
      desc: "",
      url: "",
      option: {
        // user_agent: "",
        with_proxy: false,
        self_proxy: false,
      },
    },
  });

  const isRemote = watch("type") === "remote";

  const commonProps = {
    autoComplete: "off",
    autoCorrect: "off",
    fullWidth: true,
  };

  const handleProfileSelected = (content: string) => {
    localProfile.current = content;

    setLocalProfileMessage("");
  };

  const onSubmit = handleSubmit(async (form) => {
    try {
      if (isRemote) {
        await createProfile(form);
      } else {
        if (localProfile.current) {
          await createProfile(form, localProfile.current);
        } else {
          setLocalProfileMessage("Not selected profile");

          return;
        }
      }

      setTimeout(() => reset(), 300);

      setOpen(false);
    } finally {
    }
  });

  return (
    <>
      <FloatingButton onClick={() => setOpen(true)}>
        <Add className="!size-8 absolute" />
      </FloatingButton>

      <BaseDialog
        title={t("Create Profile")}
        open={open}
        onClose={() => setOpen(false)}
        onOk={onSubmit}
        divider
      >
        <div className="flex flex-col gap-4 pt-2 pb-2">
          <SelectElement
            label={t("Type")}
            name="type"
            control={control}
            {...commonProps}
            size="small"
            required
            options={[
              {
                id: "remote",
                label: t("Remote Profile"),
              },
              {
                id: "local",
                label: t("Local Profile"),
              },
            ]}
          />

          <TextFieldElement
            label={t("Name")}
            name="name"
            control={control}
            size="small"
            fullWidth
            required
          />

          <TextFieldElement
            label={t("Descriptions")}
            name="desc"
            control={control}
            {...commonProps}
            size="small"
          />

          {isRemote ? (
            <>
              <TextFieldElement
                label={t("Subscription URL")}
                name="url"
                control={control}
                {...commonProps}
                size="small"
                multiline
                required
              />

              <TextFieldElement
                label="User Agent"
                name="option.user_agent"
                control={control}
                {...commonProps}
                size="small"
                placeholder={`clash-nyanpasu/v${version}`}
              />

              <TextFieldElement
                label={t("Update Interval")}
                name="option.update_interval"
                control={control}
                {...commonProps}
                size="small"
                type="number"
                InputProps={{
                  inputProps: { min: 0 },
                  endAdornment: (
                    <InputAdornment position="end">mins</InputAdornment>
                  ),
                }}
              />

              <Controller
                name="option.with_proxy"
                control={control}
                render={({ field }) => (
                  <LabelSwitch
                    label={t("Use System Proxy")}
                    checked={field.value}
                    {...field}
                  />
                )}
              />

              <Controller
                name="option.self_proxy"
                control={control}
                render={({ field }) => (
                  <LabelSwitch
                    label={t("Use Clash Proxy")}
                    checked={field.value}
                    {...field}
                  />
                )}
              />
            </>
          ) : (
            <>
              <ReadProfile onSelected={handleProfileSelected} />

              {localProfileMessage && (
                <div className="text-red-500 ml-2">{localProfileMessage}</div>
              )}
            </>
          )}
        </div>
      </BaseDialog>
    </>
  );
};

export default NewProfileButton;
