import { version } from "~/package.json";
import { useAsyncEffect, useReactive } from "ahooks";
import { createContext, use, useEffect, useRef, useState } from "react";
import {
  Controller,
  SelectElement,
  TextFieldElement,
  useForm,
} from "react-hook-form-mui";
import { useTranslation } from "react-i18next";
import { classNames } from "@/utils";
import { Divider, InputAdornment } from "@mui/material";
import { Profile, useClash } from "@nyanpasu/interface";
import { BaseDialog } from "@nyanpasu/ui";
import { LabelSwitch } from "../setting/modules/clash-field";
import { ProfileMonacoView, ProfileMonacoViewRef } from "./profile-monaco-view";
import { ReadProfile } from "./read-profile";

export interface ProfileDialogProps {
  profile?: Profile.Item;
  open: boolean;
  onClose: () => void;
}

export type AddProfileContextValue = {
  name: string | null;
  desc: string | null;
  url: string;
};

export const AddProfileContext = createContext<AddProfileContextValue | null>(
  null,
);

export const ProfileDialog = ({
  profile,
  open,
  onClose,
}: ProfileDialogProps) => {
  const { t } = useTranslation();

  const { createProfile, setProfiles, getProfileFile, setProfileFile } =
    useClash();

  const localProfile = useRef("");
  const addProfileCtx = use(AddProfileContext);
  const [localProfileMessage, setLocalProfileMessage] = useState("");

  const { control, watch, handleSubmit, reset, setValue } =
    useForm<Profile.Item>({
      defaultValues: profile || {
        type: "remote",
        name: addProfileCtx?.name || `New Profile`,
        desc: addProfileCtx?.desc || "",
        url: addProfileCtx?.url || "",
        option: {
          // user_agent: "",
          with_proxy: false,
          self_proxy: false,
        },
      },
    });

  useEffect(() => {
    if (addProfileCtx) {
      setValue("url", addProfileCtx.url);
      if (addProfileCtx.desc) setValue("desc", addProfileCtx.desc);
      if (addProfileCtx.name) setValue("name", addProfileCtx.name);
    }
  }, [addProfileCtx, setValue]);

  const isRemote = watch("type") === "remote";

  const isEdit = Boolean(profile);

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
    const toCreate = async () => {
      if (isRemote) {
        await createProfile(form);
      } else {
        if (localProfile.current) {
          await createProfile(form, localProfile.current);
        } else {
          // setLocalProfileMessage("Not selected profile");
          await createProfile(form, "rules: []");
        }
      }
    };

    const toUpdate = async () => {
      const value = profileMonacoViewRef.current?.getValue() || "";
      await setProfileFile(form.uid, value);
      await setProfiles(form.uid, form);
    };

    try {
      if (isEdit) {
        await toUpdate();
      } else {
        await toCreate();
      }

      setTimeout(() => reset(), 300);

      onClose();
    } finally {
    }
  });

  const profileMonacoViewRef = useRef<ProfileMonacoViewRef>(null);

  const editor = useReactive({
    value: "",
    language: "yaml",
  });

  const dialogProps = isEdit && {
    contentStyle: {
      overflow: "hidden",
      padding: 0,
    },
    full: true,
  };

  const MetaInfo = ({ className }: { className?: string }) => (
    <div className={classNames("flex flex-col gap-4 pb-2 pt-2", className)}>
      {!isEdit && (
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
      )}

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
        multiline
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
        !isEdit && (
          <>
            <ReadProfile onSelected={handleProfileSelected} />

            {localProfileMessage && (
              <div className="ml-2 text-red-500">{localProfileMessage}</div>
            )}
            <span className="px-2 text-xs">
              * {t("Select file to import or leave blank to touch new one.")}
            </span>
          </>
        )
      )}
    </div>
  );

  useAsyncEffect(async () => {
    if (profile) {
      reset(profile);
    }

    if (isEdit) {
      editor.value = await getProfileFile(profile?.uid);
    }
  }, [open]);

  return (
    <BaseDialog
      title={isEdit ? t("Edit Profile") : t("Create Profile")}
      open={open}
      onClose={() => onClose()}
      onOk={onSubmit}
      divider
      {...dialogProps}
    >
      {isEdit ? (
        <div className="flex h-full">
          <div className="min-w-72 overflow-auto pb-4 pt-4">
            <MetaInfo className="pl-4 pr-4" />
          </div>

          <Divider orientation="vertical" />

          <ProfileMonacoView
            className="w-full"
            ref={profileMonacoViewRef}
            open={open}
            value={editor.value}
            language={editor.language}
          />
        </div>
      ) : (
        <MetaInfo />
      )}
    </BaseDialog>
  );
};
