import { Divider } from "@mui/material";
import { BaseDialog, BaseDialogProps } from "@nyanpasu/ui";
import { useRef } from "react";
import { useAsyncEffect, useReactive } from "ahooks";
import { Profile, useClash } from "@nyanpasu/interface";
import { ProfileMonacoView, ProfileMonacoViewRef } from "./profile-monaco-view";
import { SelectElement, TextFieldElement, useForm } from "react-hook-form-mui";
import { useTranslation } from "react-i18next";
import { isEqual } from "lodash-es";

export interface ScriptDialogProps extends Omit<BaseDialogProps, "title"> {
  open: boolean;
  onClose: () => void;
  item?: Profile.Item;
}

export const ScriptDialog = ({
  open,
  item,
  onClose,
  ...props
}: ScriptDialogProps) => {
  const { t } = useTranslation();

  const { getProfileFile, setProfileFile, createProfile, setProfiles } =
    useClash();

  const optionTypeMapping = [
    {
      id: "js",
      value: { script: "javascript" },
      language: "javascript",
      label: t("JavaScript"),
    },
    {
      id: "lua",
      value: { script: "lua" },
      language: "lua",
      label: t("LuaScript"),
    },
    {
      id: "merge",
      value: "merge",
      language: "yaml",
      label: t("Merge"),
    },
  ];

  const preprocessing = () => {
    const result = optionTypeMapping.find((option) =>
      isEqual(option.value, item?.type),
    );

    return { ...item, type: result?.id } as Profile.Item;
  };

  const { control, watch, handleSubmit, reset } = useForm<Profile.Item>({
    defaultValues: item
      ? preprocessing()
      : {
          type: "merge",
          chains: [],
          name: "New Script",
          desc: "",
        },
  });

  const profileMonacoViewRef = useRef<ProfileMonacoViewRef>(null);

  const editor = useReactive({
    value: "",
    language: "javascript",
  });

  const handleTypeChange = () => {
    const language = optionTypeMapping.find((option) =>
      isEqual(option.id, watch("type")),
    )?.language;

    if (language) {
      editor.language = language;
    }
  };

  const isEdit = Boolean(item);

  const commonProps = {
    autoComplete: "off",
    autoCorrect: "off",
    fullWidth: true,
  };

  const onSubmit = handleSubmit(async (form) => {
    const value = profileMonacoViewRef.current?.getValue() || "";

    const type = optionTypeMapping.find((option) =>
      isEqual(option.id, form.type),
    )?.value;

    const data = {
      ...form,
      type,
    } as Profile.Item;

    try {
      if (isEdit) {
        await setProfiles(data.uid, data);

        await setProfileFile(data.uid, value);
      } else {
        await createProfile(data, value);
      }

      setTimeout(() => reset(), 300);

      onClose();
    } finally {
    }
  });

  useAsyncEffect(async () => {
    editor.value = await getProfileFile(item?.uid);

    if (item) {
      reset(item);
    } else {
      reset();
    }
  }, [open]);

  return (
    <BaseDialog
      title={isEdit ? "Edit Script" : "New Script"}
      open={open}
      onClose={() => onClose()}
      onOk={onSubmit}
      divider
      sx={{
        " .MuiDialog-paper": {
          maxWidth: "90vw",
          maxHeight: "90vh",
        },
      }}
      contentSx={{
        overflow: "auto",
        width: "90vw",
        height: "90vh",
        padding: 0,
      }}
      {...props}
    >
      <div className="flex h-full">
        <div className="pt-4 pb-4 overflow-auto">
          <div className="flex flex-col gap-4 pl-4 pr-4 pb-4">
            {!isEdit && (
              <SelectElement
                label={t("Type")}
                name="type"
                control={control}
                {...commonProps}
                size="small"
                required
                options={optionTypeMapping}
                onChange={() => handleTypeChange()}
              />
            )}

            <TextFieldElement
              label={t("Name")}
              name="name"
              control={control}
              {...commonProps}
              size="small"
              required
            />

            <TextFieldElement
              label={t("Descriptions")}
              name="desc"
              control={control}
              {...commonProps}
              size="small"
            />
          </div>
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
    </BaseDialog>
  );
};
