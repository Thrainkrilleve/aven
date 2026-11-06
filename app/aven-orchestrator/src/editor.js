// CodeMirror 6 editor for the Aven IDE: .aven syntax highlighting + inline
// diagnostics (gutter markers and underlines) driven by the `check_model`
// Tauri command. Bundled offline by esbuild into dist/editor.bundle.js and
// exposed as the global `AvenEditor`.

import { EditorView, basicSetup } from "codemirror";
import { EditorState } from "@codemirror/state";
import { StreamLanguage, HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { linter, lintGutter } from "@codemirror/lint";
import { tags as t } from "@lezer/highlight";

// A small stream tokenizer for Aven: keywords, builtins, constants, numbers,
// strings, block/line comments, operators.
const avenLanguage = StreamLanguage.define({
  startState: () => ({ block: false }),
  token(stream, state) {
    if (state.block) {
      if (stream.match(/.*?\*\//)) state.block = false;
      else stream.skipToEnd();
      return "comment";
    }
    if (stream.match(/\/\*/)) {
      state.block = true;
      return "comment";
    }
    if (stream.match(/\/\/.*/)) return "comment";
    if (stream.match(/"(?:[^"\\]|\\.)*"/)) return "string";
    if (stream.match(/\b(relation|forward|solve|signal|unit|using|with|minimize|maximize|integer|int|external|ipc|shm|native)\b/))
      return "keyword";
    if (stream.match(/\b(sqrt|sin|cos|tan|exp|ln|abs|ceil|floor|round|sign|min|max)\b/))
      return "variableName.function";
    if (stream.match(/\b(pi|tau|e|PI)\b/)) return "atom";
    if (stream.match(/[0-9]+(?:\.[0-9]+)?(?:[eE][-+]?[0-9]+)?/)) return "number";
    if (stream.match(/<=|>=|==|[-+*/^<>=]/)) return "operator";
    stream.next();
    return null;
  },
});

const avenHighlight = HighlightStyle.define([
  { tag: t.keyword, color: "#c792ea" },
  { tag: t.function(t.variableName), color: "#82aaff" },
  { tag: t.atom, color: "#f5b451" },
  { tag: t.number, color: "#f5b451" },
  { tag: t.string, color: "#4ade80" },
  { tag: t.comment, color: "#5f6b7d", fontStyle: "italic" },
  { tag: t.operator, color: "#89ddff" },
]);

const avenTheme = EditorView.theme(
  {
    "&": { color: "#e6ebf2", backgroundColor: "#0a0f17", height: "100%", fontSize: "0.86rem" },
    ".cm-content": { fontFamily: "'JetBrains Mono', ui-monospace, Menlo, Consolas, monospace", caretColor: "#4fd1e8", padding: "1rem 0" },
    ".cm-scroller": { fontFamily: "'JetBrains Mono', ui-monospace, Menlo, Consolas, monospace", lineHeight: "1.6" },
    ".cm-gutters": { backgroundColor: "#0a0f17", color: "#4a5568", border: "none" },
    ".cm-activeLine": { backgroundColor: "rgba(255,255,255,0.03)" },
    ".cm-activeLineGutter": { backgroundColor: "rgba(255,255,255,0.03)" },
    "&.cm-focused": { outline: "none" },
    ".cm-lintRange-error": { textDecoration: "underline wavy #f87171" },
  },
  { dark: true }
);

// Linter that maps the backend's Diagnostic{start,end} to CodeMirror ranges.
function avenLinter(getInvoke) {
  return linter(
    async (view) => {
      const invoke = getInvoke();
      if (!invoke) return [];
      const source = view.state.doc.toString();
      let diags;
      try {
        diags = await invoke("check_model", { source });
      } catch {
        return [];
      }
      const doc = view.state.doc;
      const clampLine = (n) => Math.min(Math.max(n || 1, 1), doc.lines);
      const posAt = (line, col) => {
        const l = doc.line(clampLine(line));
        return Math.min(l.from + Math.max((col || 1) - 1, 0), l.to);
      };
      return (diags || []).map((d) => {
        const from = posAt(d.start.line, d.start.col);
        let to = d.end ? posAt(d.end.line, d.end.col) : from;
        if (to <= from) {
          // No span yet: underline to the end of the line so it is visible.
          to = doc.line(clampLine(d.start.line)).to;
          if (to <= from) to = Math.min(from + 1, doc.length);
        }
        return {
          from,
          to,
          severity: d.severity === "warning" ? "warning" : "error",
          message: d.message,
        };
      });
    },
    { delay: 350 }
  );
}

// Mount an editor into `parent`. Returns an object with a `value` get/set
// property (so existing code that used `editor.value` keeps working), `focus`,
// and the underlying view.
export function mount(parent, opts) {
  const { doc = "", onChange, getInvoke } = opts || {};
  const updateListener = EditorView.updateListener.of((u) => {
    if (u.docChanged && onChange) onChange(view.state.doc.toString());
  });
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        basicSetup,
        avenLanguage,
        syntaxHighlighting(avenHighlight),
        avenTheme,
        lintGutter(),
        avenLinter(getInvoke),
        updateListener,
      ],
    }),
  });
  return {
    get value() {
      return view.state.doc.toString();
    },
    set value(v) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: v ?? "" } });
    },
    focus() {
      view.focus();
    },
    view,
  };
}
