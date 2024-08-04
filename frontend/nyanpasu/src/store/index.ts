import { atom } from "jotai";
import { SortType } from "@/components/proxies/utils";
import { LogMessage } from "@nyanpasu/interface";

const atomWithLocalStorage = <T>(key: string, initialValue: T) => {
  const getInitialValue = (): T => {
    const item = localStorage.getItem(key);

    return item ? JSON.parse(item) : initialValue;
  };

  const baseAtom = atom<T>(getInitialValue());

  const derivedAtom = atom(
    (get) => get(baseAtom),
    (get, set, update: T | ((prev: T) => T)) => {
      const nextValue =
        typeof update === "function"
          ? (update as (prev: T) => T)(get(baseAtom))
          : update;

      set(baseAtom, nextValue);

      localStorage.setItem(key, JSON.stringify(nextValue));
    },
  );

  return derivedAtom;
};

export const proxyGroupAtom = atomWithLocalStorage<{
  selector: number | null;
}>("proxyGroupAtom", {
  selector: 0,
});

export const proxyGroupSortAtom = atomWithLocalStorage<SortType>(
  "proxyGroupSortAtom",
  SortType.Default,
);

export const themeMode = atomWithLocalStorage<"light" | "dark">(
  "themeMode",
  "light",
);

export const atomLogData = atomWithLocalStorage<LogMessage[]>(
  "atomLogData",
  [],
);

export const atomEnableLog = atomWithLocalStorage<boolean>(
  "atomEnableLog",
  true,
);

export const atomIsDrawer = atom<boolean>();

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
