// .aven highlighting. Keywords match the lexer in src/lexer.rs.
// Units are the words (and / or ^ between them) after `:` or after a number,
// so `signal v : km/h` and `v = 90 km/h` both light up.

import { StreamLanguage } from "@codemirror/language"
import { tags } from "@lezer/highlight"

const KEYWORDS = new Set([
  "relation",
  "forward",
  "solve",
  "with",
  "using",
  "external",
  "ipc",
  "shm",
  "native",
  "unit",
  "signal",
  "minimize",
  "maximize",
  "int",
  "integer",
])

export function startState() {
  return { afterColon: false, afterNumber: false, block: false }
}

function unitContext(state) {
  return state.afterColon || state.afterNumber
}

export function tokenAven(stream, state) {
  if (stream.sol()) {
    state.afterColon = false
    state.afterNumber = false
  }

  if (state.block) {
    while (!stream.eol()) {
      if (stream.match("*/")) {
        state.block = false
        break
      }
      stream.next()
    }
    return "comment"
  }

  if (stream.eatSpace()) return null

  if (stream.match("//")) {
    stream.skipToEnd()
    state.afterColon = false
    state.afterNumber = false
    return "comment"
  }

  if (stream.match("/*")) {
    state.block = true
    state.afterColon = false
    state.afterNumber = false
    while (!stream.eol()) {
      if (stream.match("*/")) {
        state.block = false
        break
      }
      stream.next()
    }
    return "comment"
  }

  if (stream.peek() === '"') {
    stream.next()
    while (!stream.eol()) {
      const c = stream.next()
      if (c === "\\") {
        stream.next()
        continue
      }
      if (c === '"') break
    }
    state.afterColon = false
    state.afterNumber = false
    return "string"
  }

  if (stream.match(/^(?:\d+\.\d+|\d+)(?:[eE][+-]?\d+)?/)) {
    state.afterNumber = true
    state.afterColon = false
    return "number"
  }

  if (stream.match(/^[A-Za-z_][A-Za-z0-9_]*/)) {
    const word = stream.current()
    if (KEYWORDS.has(word)) {
      state.afterColon = false
      state.afterNumber = false
      return "keyword"
    }
    const unit = unitContext(state)
    if (!unit) state.afterNumber = false
    return unit ? "unit" : "variableName"
  }

  const ch = stream.peek()
  if (unitContext(state) && (ch === "/" || ch === "^" || ch === "·")) {
    stream.next()
    return "unit"
  }

  stream.next()
  if (ch === ":") {
    state.afterColon = true
    state.afterNumber = false
    return "operator"
  }
  state.afterColon = false
  state.afterNumber = false
  return "punctuation"
}

export const avenLanguage = StreamLanguage.define({
  name: "aven",
  startState,
  token: tokenAven,
  tokenTable: {
    keyword: tags.keyword,
    comment: tags.lineComment,
    string: tags.string,
    number: tags.number,
    operator: tags.operator,
    variableName: tags.variableName,
    unit: tags.typeName,
    punctuation: tags.punctuation,
  },
  languageData: {
    commentTokens: { line: "//", block: { open: "/*", close: "*/" } },
  },
})

/** Split one line into {text, kind} pairs. `kind` null is whitespace. */
export function tokensOf(line, state = startState()) {
  let i = 0
  const out = []
  const stream = {
    sol: () => i === 0,
    eol: () => i >= line.length,
    peek: () => (i < line.length ? line[i] : undefined),
    next: () => (i < line.length ? line[i++] : undefined),
    eatSpace() {
      const from = i
      while (i < line.length && (line[i] === " " || line[i] === "\t")) i++
      return i > from
    },
    match(pat) {
      if (typeof pat === "string") {
        if (line.startsWith(pat, i)) {
          i += pat.length
          return true
        }
        return false
      }
      const m = pat.exec(line.slice(i))
      if (m) {
        i += m[0].length
        return m
      }
      return null
    },
    skipToEnd() {
      i = line.length
    },
    current() {
      return line.slice(start, i)
    },
  }
  let start = 0
  while (i < line.length) {
    start = i
    const kind = tokenAven(stream, state)
    const text = line.slice(start, i)
    if (!text) break
    out.push({ text, kind })
  }
  return { tokens: out, state }
}
