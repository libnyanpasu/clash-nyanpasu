import { BasePage, DialogRef } from "@/components/base";
import { ProfileItem } from "@/components/profile/profile-item";
import { ProfileMore } from "@/components/profile/profile-more";
import {
  ProfileViewer,
  ProfileViewerRef,
} from "@/components/profile/profile-viewer";
import { ConfigViewer } from "@/components/setting/mods/config-viewer";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { useProfiles } from "@/hooks/use-profiles";
import { closeAllConnections } from "@/services/api";
import {
  deleteProfile,
  enhanceProfiles,
  getProfiles,
  getRuntimeLogs,
  importProfile,
  reorderProfile,
  updateProfile,
} from "@/services/cmds";
import { atomLoadingCache } from "@/store";
import {
  DndContext,
  DragEndEvent,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
} from "@dnd-kit/sortable";
import {
  ClearRounded,
  ContentCopyRounded,
  LocalFireDepartmentRounded,
  RefreshRounded,
  TextSnippetOutlined,
} from "@mui/icons-material";
import LoadingButton from "@mui/lab/LoadingButton";
import { Box, Button, Grid, IconButton, Stack, TextField } from "@mui/material";
import { useLockFn } from "ahooks";
import { useSetAtom } from "jotai";
import { throttle } from "lodash-es";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation } from "react-router-dom";
import useSWR, { mutate } from "swr";

export default function ProfilePage() {
  const { t } = useTranslation();
  const location = useLocation();

  const [url, setUrl] = useState("");
  const [disabled, setDisabled] = useState(false);
  const [activating, setActivating] = useState("");
  const [loading, setLoading] = useState(false);
  const sensors = useSensors(
    useSensor(PointerSensor),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const {
    profiles = {},
    activateSelected,
    patchProfiles,
    mutateProfiles,
  } = useProfiles();

  const { data: chainLogs = {}, mutate: mutateLogs } = useSWR(
    "getRuntimeLogs",
    getRuntimeLogs,
  );

  const chain = profiles.chain || [];
  const viewerRef = useRef<ProfileViewerRef>(null);
  const configRef = useRef<DialogRef>(null);

  // distinguish type
  const { regularItems, enhanceItems } = useMemo(() => {
    const items = profiles.items || [];
    const chain = profiles.chain || [];

    const type1 = ["local", "remote"];
    const type2 = ["merge", "script"];

    const regularItems = items.filter((i) => i && type1.includes(i.type!));
    const restItems = items.filter((i) => i && type2.includes(i.type!));
    const restMap = Object.fromEntries(restItems.map((i) => [i.uid, i]));
    const enhanceItems = chain
      .map((i) => restMap[i]!)
      .filter(Boolean)
      .concat(restItems.filter((i) => !chain.includes(i.uid)));

    return { regularItems, enhanceItems };
  }, [profiles]);

  useEffect(() => {
    if (location.state != null) {
      console.log(location.state.scheme);
      viewerRef.current?.create();
    }
  }, [location]);

  const onImport = async () => {
    if (!url) return;
    setLoading(true);

    try {
      await importProfile(url);
      useNotification({
        title: t("Success"),
        body: "Successfully import profile.",
        type: NotificationType.Success,
      });
      setUrl("");
      setLoading(false);

      getProfiles().then((newProfiles) => {
        mutate("getProfiles", newProfiles);

        const remoteItem = newProfiles.items?.find((e) => e.type === "remote");
        if (!newProfiles.current && remoteItem) {
          const current = remoteItem.uid;
          patchProfiles({ current });
          mutateLogs();
          setTimeout(() => activateSelected(), 2000);
        }
      });
    } catch (err: any) {
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
      setLoading(false);
    } finally {
      setDisabled(false);
      setLoading(false);
    }
  };

  const onDragEnd = async (event: DragEndEvent) => {
    const { active, over } = event;
    if (over) {
      if (active.id !== over.id) {
        await reorderProfile(active.id.toString(), over.id.toString());
        mutateProfiles();
      }
    }
  };

  const onSelect = useLockFn(async (current: string, force: boolean) => {
    if (!force && current === profiles.current) return;
    // 避免大多数情况下loading态闪烁
    const reset = setTimeout(() => setActivating(current), 100);
    try {
      await patchProfiles({ current });
      mutateLogs();
      closeAllConnections();
      setTimeout(() => activateSelected(), 2000);
      useNotification({
        title: t("Success"),
        body: "Refresh Clash Config",
        type: NotificationType.Success,
      });
    } catch (err: any) {
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    } finally {
      clearTimeout(reset);
      setActivating("");
    }
  });

  const onEnhance = useLockFn(async () => {
    try {
      await enhanceProfiles();
      mutateLogs();
      useNotification({
        title: t("Success"),
        body: "Refresh Clash Config",
        type: NotificationType.Success,
      });
    } catch (err: any) {
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    }
  });

  const onEnable = useLockFn(async (uid: string) => {
    if (chain.includes(uid)) return;
    const newChain = [...chain, uid];
    await patchProfiles({ chain: newChain });
    mutateLogs();
  });

  const onDisable = useLockFn(async (uid: string) => {
    if (!chain.includes(uid)) return;
    const newChain = chain.filter((i) => i !== uid);
    await patchProfiles({ chain: newChain });
    mutateLogs();
  });

  const onDelete = useLockFn(async (uid: string) => {
    try {
      await onDisable(uid);
      await deleteProfile(uid);
      mutateProfiles();
      mutateLogs();
    } catch (err: any) {
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    }
  });

  const onMoveTop = useLockFn(async (uid: string) => {
    if (!chain.includes(uid)) return;
    const newChain = [uid].concat(chain.filter((i) => i !== uid));
    await patchProfiles({ chain: newChain });
    mutateLogs();
  });

  const onMoveEnd = useLockFn(async (uid: string) => {
    if (!chain.includes(uid)) return;
    const newChain = chain.filter((i) => i !== uid).concat([uid]);
    await patchProfiles({ chain: newChain });
    mutateLogs();
  });

  // 更新所有配置
  const setLoadingCache = useSetAtom(atomLoadingCache);
  const onUpdateAll = useLockFn(async () => {
    const throttleMutate = throttle(mutateProfiles, 2000, {
      trailing: true,
    });
    const updateOne = async (uid: string) => {
      try {
        await updateProfile(uid);
        throttleMutate();
      } finally {
        setLoadingCache((cache) => ({ ...cache, [uid]: false }));
      }
    };

    return new Promise((resolve) => {
      setLoadingCache((cache) => {
        // 获取没有正在更新的配置
        const items = regularItems.filter(
          (e) => e.type === "remote" && !cache[e.uid],
        );
        const change = Object.fromEntries(items.map((e) => [e.uid, true]));

        Promise.allSettled(items.map((e) => updateOne(e.uid))).then(resolve);
        return { ...cache, ...change };
      });
    });
  });

  const onCopyLink = async () => {
    const text = await navigator.clipboard.readText();
    if (text) setUrl(text);
  };

  const [sectionOverflowStatus, setSectionOverflowStatus] = useState(false);

  return (
    <BasePage
      title={t("Profiles")}
      sectionStyle={{
        overflow: sectionOverflowStatus ? "hidden" : "auto",
      }}
      header={
        <Box sx={{ mt: 1, display: "flex", alignItems: "center", gap: 1 }}>
          <IconButton
            size="small"
            color="inherit"
            title={t("Update All Profiles")}
            onClick={onUpdateAll}
          >
            <RefreshRounded />
          </IconButton>

          <IconButton
            size="small"
            color="inherit"
            title={t("View Runtime Config")}
            onClick={() => configRef.current?.open()}
          >
            <TextSnippetOutlined />
          </IconButton>

          <IconButton
            size="small"
            color="primary"
            title={t("Reactivate Profiles")}
            onClick={onEnhance}
          >
            <LocalFireDepartmentRounded />
          </IconButton>
        </Box>
      }
    >
      <Stack direction="row" spacing={1} sx={{ mb: 2 }}>
        <TextField
          hiddenLabel
          fullWidth
          size="small"
          value={url}
          variant="outlined"
          autoComplete="off"
          spellCheck="false"
          onChange={(e) => setUrl(e.target.value)}
          sx={{ input: { py: 0.65, px: 1.25 } }}
          placeholder={t("Profile URL")}
          InputProps={{
            sx: {
              borderRadius: 4,
              pr: 1,
            },
            endAdornment: !url ? (
              <IconButton
                size="small"
                sx={{ p: 0.5 }}
                title={t("Paste")}
                onClick={onCopyLink}
              >
                <ContentCopyRounded fontSize="inherit" />
              </IconButton>
            ) : (
              <IconButton
                size="small"
                sx={{ p: 0.5 }}
                title={t("Clear")}
                onClick={() => setUrl("")}
              >
                <ClearRounded fontSize="inherit" />
              </IconButton>
            ),
          }}
        />
        <LoadingButton
          disabled={!url || disabled}
          loading={loading}
          variant="contained"
          size="small"
          onClick={onImport}
          sx={{
            borderRadius: 4,
          }}
        >
          {t("Import")}
        </LoadingButton>
        <Button
          variant="contained"
          size="small"
          onClick={() => viewerRef.current?.create()}
          sx={{
            borderRadius: 4,
          }}
        >
          {t("New")}
        </Button>
      </Stack>

      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragEnd={onDragEnd}
        onDragStart={() => setSectionOverflowStatus(true)}
        onDragOver={() => setSectionOverflowStatus(false)}
      >
        <Box sx={{ mb: 4.5 }}>
          <Grid container spacing={{ xs: 3, lg: 3 }}>
            <SortableContext
              items={regularItems.map((x) => {
                return x.uid;
              })}
            >
              {regularItems.map((item) => (
                <Grid item xs={12} md={6} lg={4} xl={3} key={item.file}>
                  <ProfileItem
                    id={item.uid}
                    selected={profiles.current === item.uid}
                    activating={activating === item.uid}
                    itemData={item}
                    onSelect={(f) => onSelect(item.uid, f)}
                    onEdit={() => viewerRef.current?.edit(item)}
                  />
                </Grid>
              ))}
            </SortableContext>
          </Grid>
        </Box>
      </DndContext>

      {enhanceItems.length > 0 && (
        <Grid container spacing={{ xs: 2, lg: 3 }}>
          {enhanceItems.map((item) => (
            <Grid item xs={12} sm={6} md={4} lg={3} key={item.file}>
              <ProfileMore
                selected={!!chain.includes(item.uid)}
                itemData={item}
                enableNum={chain.length || 0}
                logInfo={chainLogs[item.uid]}
                onEnable={() => onEnable(item.uid)}
                onDisable={() => onDisable(item.uid)}
                onDelete={() => onDelete(item.uid)}
                onMoveTop={() => onMoveTop(item.uid)}
                onMoveEnd={() => onMoveEnd(item.uid)}
                onEdit={() => viewerRef.current?.edit(item)}
              />
            </Grid>
          ))}
        </Grid>
      )}

      <ProfileViewer
        ref={viewerRef}
        url={location.state?.subscribe?.url as string | undefined}
        name={location.state?.subscribe?.name as string | undefined}
        desc={location.state?.subscribe?.desc as string | undefined}
        onChange={() => mutateProfiles()}
      />
      <ConfigViewer ref={configRef} />
    </BasePage>
  );
}
