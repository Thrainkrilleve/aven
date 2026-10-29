// Desktop editor. Rebuild the bundle with `npm run build:ui` from
// app/aven-orchestrator. The bundle in dist/ is what the window loads.

import { EditorState } from "@codemirror/state"
import { EditorView, keymap, lineNumbers, drawSelection, highlightActiveLine } from "@codemirror/view"
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands"
import { bracketMatching, HighlightStyle, syntaxHighlighting } from "@codemirror/language"
import { linter } from "@codemirror/lint"
import { tags } from "@lezer/highlight"
import { invoke } from "@tauri-apps/api/core"
import { avenLanguage } from "./aven-mode.mjs"

const STARTER = `// distance = speed * time
// 90 km/h for 2.5 h. Edit the numbers, or blank one and the solver fills it in.

signal speed : km/h
signal time : h

relation Travel {
    distance = speed * time
}

solve Travel with {
    speed = 90 km/h
    time = 2.5 h
}
`

const avenHighlight = HighlightStyle.define([
  { tag: tags.keyword, color: "#7dd3fc", fontWeight: "600" },
  { tag: tags.lineComment, color: "#64748b", fontStyle: "italic" },
  { tag: tags.number, color: "#fbbf24" },
  { tag: tags.string, color: "#86efac" },
  { tag: tags.typeName, color: "#34d399" },
  { tag: tags.variableName, color: "#e2e8f0" },
  { tag: tags.operator, color: "#94a3b8" },
  { tag: tags.punctuation, color: "#94a3b8" },
])

const theme = EditorView.theme({
  "&": {
    height: "100%",
    background: "#080c14",
    color: "#e2e8f0",
    fontSize: "14px",
  },
  ".cm-scroller": {
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
    lineHeight: "1.55",
  },
  ".cm-content": { padding: "12px 0" },
  ".cm-gutters": {
    background: "#0b101a",
    color: "#475569",
    border: "none",
  },
  ".cm-activeLine": { background: "rgba(56, 189, 248, 0.06)" },
  ".cm-activeLineGutter": { background: "transparent", color: "#94a3b8" },
  "&.cm-focused": { outline: "none" },
  ".cm-diagnostic-error": { borderLeft: "3px solid #f87171" },
  ".cm-lintRange-error": {
    background: "rgba(248, 113, 113, 0.16)",
    textDecoration: "underline wavy #f87171",
  },
})

function setStatus(text, bad) {
  const el = document.getElementById("status")
  if (!el) return
  el.textContent = text
  el.dataset.state = bad ? "bad" : "ok"
}

function span(doc, line, col) {
  if (!line || line < 1) return 0
  const info = doc.line(Math.min(line, doc.lines))
  const offset = Math.max(0, (col || 1) - 1)
  return Math.min(info.from + offset, info.to)
}

let request = 0

const checkLinter = linter(async (view) => {
  const id = ++request
  const text = view.state.doc.toString()
  let rows
  try {
    rows = await invoke("ide_check", { source: text })
  } catch {
    if (id === request) setStatus("Editor ready")
    return []
  }
  if (id !== request || view.state.doc.toString() !== text) return []
  if (!rows.length) {
    setStatus("No unit errors")
    return []
  }
  setStatus(rows[0].message, true)
  return rows.map((row) => ({
    from: span(view.state.doc, row.start_line, row.start_col),
    to: span(view.state.doc, row.end_line, row.end_col),
    severity: row.severity === "warning" ? "warning" : "error",
    message: row.message,
  }))
}, { delay: 300 })

const parent = document.getElementById("editor")
if (parent) {
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: STARTER,
      extensions: [
        lineNumbers(),
        highlightActiveLine(),
        drawSelection(),
        history(),
        bracketMatching(),
        keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
        avenLanguage,
        syntaxHighlighting(avenHighlight),
        theme,
        checkLinter,
        EditorView.lineWrapping,
      ],
    }),
  })
  view.focus()
}
