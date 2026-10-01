/** Rust comment/literal lexer shared by metric and test-region scanning. */
/**
 * Recognizes a raw string opener: `r"`, `r#"`, `r##"`, ...; `br"`, `br#"`,
 * ...; or the raw C string forms `cr"`, `cr#"`, ...
 */
const RAW_STRING_START_RE = /^(?:cr|br|r)(#*)"/;

/**
 * Recognizes a Rust character literal starting at `'`: `'x'`, `'\n'`, `'\''`,
 * `'\\'`, `'\x41'`, `'\u{7FFF}'`. Anything else starting with `'` (`'a`,
 * `'static`, `'_`) is a lifetime, not a char literal, and must not match.
 */
const CHAR_LITERAL_RE =
  /^'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^'\\])'/;

/** True when `raw[index - 1]` (if any) is not an identifier character. */
function isWordBoundaryBefore(raw: string, index: number): boolean {
  return index === 0 || !/[A-Za-z0-9_]/.test(raw[index - 1]);
}

// Lexer state carried across lines: block comments nest (Rust allows
// `/* /* */ */`), so a depth counter is tracked instead of a boolean; raw
// strings (`r"..."`, `r#"..."#`, ...) can also span lines, so the number of
// `#` needed to close one is carried the same way (-1 means "not in one").
export type LexState = {
  blockDepth: number;
  rawStringHashes: number;
};

export function createLexState(): LexState {
  return { blockDepth: 0, rawStringHashes: -1 };
}

// Lex `//`, `/* */`, strings, and raw strings in source order so a `//`/`/*`
// inside a string, raw string, or comment never opens a new comment, and a
// `"` inside a comment or raw-string body never opens a string. Inside a
// normal string, only an even run of backslashes lets a `"` close it
// (escapeNext tracks the run one char at a time). A `'` only opens a char
// literal when it is actually shaped like one; otherwise (`'a`, `'static`,
// `'_`) it is a lifetime and leaves all state alone. Literal *contents* are
// replaced with nothing (delimiters are kept), so the returned code never
// carries text or braces that only exist inside a literal. A comment is
// replaced with one space where it starts, because Rust reads it as a token
// separator: `pub/* x */static` must stay two words. Lines are lexed one at
// a time, so a comment spanning lines keeps their boundaries too. Shared by
// `scanFile` and by the brace/item scanning below so both agree on what
// counts as real code.
export function stripCommentsAndLiterals(
  raw: string,
  state: LexState,
): string {
  let code = "";
  let inDouble = false;
  let escapeNext = false;

  for (let j = 0; j < raw.length; j++) {
    const ch = raw[j];

    if (state.rawStringHashes >= 0) {
      if (
        ch === '"' &&
        raw.slice(j + 1, j + 1 + state.rawStringHashes) ===
          "#".repeat(state.rawStringHashes)
      ) {
        code += ch + raw.slice(j + 1, j + 1 + state.rawStringHashes);
        j += state.rawStringHashes;
        state.rawStringHashes = -1;
      }
      continue;
    }

    if (state.blockDepth > 0) {
      if (ch === "/" && raw[j + 1] === "*") {
        state.blockDepth += 1;
        j++;
      } else if (ch === "*" && raw[j + 1] === "/") {
        state.blockDepth -= 1;
        j++;
      }
      continue;
    }

    if (inDouble) {
      if (escapeNext) {
        escapeNext = false;
      } else if (ch === "\\") {
        escapeNext = true;
      } else if (ch === '"') {
        inDouble = false;
        code += ch;
      }
      continue;
    }

    if (
      (ch === "r" ||
        (ch === "b" && raw[j + 1] === "r") ||
        (ch === "c" && raw[j + 1] === "r")) &&
      isWordBoundaryBefore(raw, j)
    ) {
      const rawStart = RAW_STRING_START_RE.exec(raw.slice(j));
      if (rawStart) {
        code += rawStart[0];
        state.rawStringHashes = rawStart[1].length;
        j += rawStart[0].length - 1;
        continue;
      }
    }

    if (ch === "'") {
      const charLiteral = CHAR_LITERAL_RE.exec(raw.slice(j));
      if (charLiteral) {
        // Delimiters kept, content blanked: a char literal like `'{'` must
        // not feed a stray brace into brace/item scanning either.
        code += "''";
        j += charLiteral[0].length - 1;
        continue;
      }
      // Lifetime tick (`'a`, `'static`, `'_`): not a string delimiter.
      code += ch;
      continue;
    }

    if (ch === '"') {
      inDouble = true;
      code += ch;
      continue;
    }

    if (ch === "/" && raw[j + 1] === "/") {
      code += " ";
      break;
    }
    if (ch === "/" && raw[j + 1] === "*") {
      code += " ";
      state.blockDepth = 1;
      j++;
      continue;
    }

    code += ch;
  }

  return code;
}
