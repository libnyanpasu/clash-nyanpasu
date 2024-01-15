import type { Highlighter } from "shikiji";
import { getHighlighterCore } from "shikiji/core";

import minLight from "shikiji/themes/min-light.mjs";
import nord from "shikiji/themes/nord.mjs";
import getWasm from "shikiji/wasm";

let shiki: Highlighter | null = null;

export async function getShikiSingleton() {
  if (!shiki) {
    shiki = await getHighlighterCore({
      themes: [nord, minLight],
      langs: [],

      loadWasm: getWasm,
    });
  }
  return shiki;
}

export async function formatAnsi(str: string) {
  const instance = await getShikiSingleton();
  return instance.codeToHtml(str, {
    lang: "ansi",
    themes: {
      dark: "nord",
      light: "min-light",
    },
  });
}
