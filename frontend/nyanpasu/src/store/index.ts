import { atom } from "jotai";

export const proxyGroupAtom = atom<{
  selector: number | null;
}>({
  selector: 0,
});

export const themeMode = atom<"light" | "dark">("light");

export const atomLogData = atom<ILogItem[]>([]);

export const atomEnableLog = atom<boolean>(false);

// save the state of each profile item loading
export const atomLoadingCache = atom<Record<string, boolean>>({});

// save update state
export const atomUpdateState = atom<boolean>(false);

interface IConnectionSetting {
  layout: "table" | "list";
}

export const atomConnectionSetting = atom<IConnectionSetting>({
  layout: "table",
});

// export const themeSchemeAtom = atom<MDYTheme["schemes"] | null>(null);
