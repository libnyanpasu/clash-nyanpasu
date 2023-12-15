import { getHighlighter, type Highlighter } from "shikiji";

let shiki: Highlighter | null = null;

export async function getShikiSingleton() {
  if (!shiki) {
    shiki = await getHighlighter({
      themes: ["nord", "min-light"],
      langs: ["ansi"],
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
