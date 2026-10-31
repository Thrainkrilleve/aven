    const invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
    let editor;
    const editorHost = document.getElementById("editor-host");
    const out = document.getElementById("out");
    const statusEl = document.getElementById("status");
    const tplSel = document.getElementById("templates");
    const fileNameEl = document.getElementById("file-name");
    const dirtyEl = document.getElementById("dirty");
    let savedText = "";

    function refreshDirty() {
      dirtyEl.hidden = editor.value === savedText;
    }
    function markSaved(text, name) {
      savedText = text;
      fileNameEl.textContent = name || "Untitled";
      refreshDirty();
    }
    function discardIfDirty() {
      if (editor.value === savedText) return true;
      return window.confirm("Discard unsaved changes?");
    }

    function esc(s) { return String(s).replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c])); }
    function fmt(v) { return Number.isFinite(v) ? (Math.abs(v) >= 1000 || v === Math.trunc(v) ? v.toLocaleString(undefined, { maximumFractionDigits: 2 }) : v.toLocaleString(undefined, { maximumFractionDigits: 4 })) : "—"; }
    function setStatus(text, cls) { statusEl.textContent = text; statusEl.className = "status " + (cls || ""); }

    function valuesTable(r) {
      const shown = Object.fromEntries(r.shown || []);
      const units = Object.fromEntries(r.units || []);
      const solving = new Set(r.solving || []);
      const bound = new Set(r.bound || []);
      const canRetarget = bound.size > 0;
      const rows = (r.values || []).map(([k, v]) => {
        const n = Object.prototype.hasOwnProperty.call(shown, k) ? shown[k] : v;
        const unit = units[k] ? ` <span class="unit">${esc(units[k])}</span>` : "";
        const name = `<button class="link" data-why="${esc(k)}">${esc(k)}</button>`;
        const editable = bound.has(k);
        const value = editable
          ? `<input class="slot" data-bind="${esc(k)}" value="${esc(String(n))}" aria-label="${esc(k)} value" />`
          : fmt(n);
        let action = "";
        if (canRetarget && solving.has(k)) action = `<span class="solving">solving</span>`;
        else if (editable) action = `<button class="tiny" data-solve="${esc(k)}">Solve for this</button>`;
        return `<tr><td>${name}</td><td class="val">${value}${unit}</td><td class="act">${action}</td></tr>`;
      }).join("");
      return `<table>${rows}</table><div class="tree" id="why-tree"></div>`;
    }

    // Inline diagnostics are now drawn by CodeMirror's linter (gutter marker +
    // wavy underline), re-run automatically on edit. This stub keeps the old
    // call sites harmless; the results panel still shows the message text.
    function renderHighlight() {}

    let requestId = 0;
    let lastDiag = null;
    function begin(label) {
      const id = ++requestId;
      const source = editor.value;
      out.innerHTML = "";
      out.appendChild(statusEl);
      setStatus(label);
      return { id, source };
    }
    function current(id) { return id === requestId; }
    function failureText(d) {
      if (typeof d === "string" && d) return d;
      if (d && typeof d.message === "string" && d.message) return d.message;
      return "Could not run this model";
    }
    function showFailure(id, d) {
      if (!current(id)) return;
      setStatus(failureText(d), "bad");
      if (d && d.start && d.start.line) renderHighlight(editor.value, d);
      pushGuide("Heads up", mismatchNote(editor.value) + insightForDiag(failureText(d)));
    }

    function showSolved(r) {
      setStatus("Solved", "ok");
      renderHighlight(editor.value, null);
      out.insertAdjacentHTML("beforeend", valuesTable(r));
      if (r.trace && r.trace.length) out.insertAdjacentHTML("beforeend", `<div class="trace">${esc(r.trace.join("\n"))}</div>`);
      wireResults();
      pushGuide("What happened", mismatchNote(editor.value) + insightForSolved(r));
    }

    async function showWhy(name) {
      const tree = document.getElementById("why-tree");
      if (!tree || !name) return;
      try {
        const text = await invoke("why_model", { source: editor.value, variable: name, depth: null });
        tree.textContent = text;
      } catch (d) {
        tree.textContent = failureText(d);
      }
    }

    async function retarget(name) {
      try {
        const next = await invoke("solve_for_variable", { source: editor.value, target: name });
        editor.value = next;
        refreshDirty();
        renderHighlight(next, null);
        await refresh();
      } catch (d) {
        const run = begin("Solving…");
        showFailure(run.id, d);
      }
    }

    function wireResults() {
      out.querySelectorAll("[data-why]").forEach((btn) => {
        btn.addEventListener("click", () => showWhy(btn.getAttribute("data-why")));
      });
      out.querySelectorAll("[data-solve]").forEach((btn) => {
        btn.addEventListener("click", () => retarget(btn.getAttribute("data-solve")));
      });
      out.querySelectorAll("[data-bind]").forEach((input) => {
        input.addEventListener("change", () => setBinding(input.getAttribute("data-bind"), input.value.trim()));
        input.addEventListener("keydown", (event) => {
          if (event.key === "Enter") input.blur();
        });
      });
    }

    function setBinding(name, raw) {
      if (!name || raw === "") return;
      const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const pattern = new RegExp("^(\\s*" + escaped + "\\s*=\\s*)(-?(?:\\d+\\.?\\d*|\\.\\d+)(?:[eE][+-]?\\d+)?)");
      const lines = editor.value.split("\n");
      let hit = false;
      const next = lines.map((line) => {
        if (hit || !pattern.test(line)) return line;
        hit = true;
        return line.replace(pattern, "$1" + raw);
      });
      if (!hit) return;
      editor.value = next.join("\n");
      refreshDirty();
      renderHighlight(editor.value, null);
      clearTimeout(refreshTimer);
      refreshTimer = setTimeout(refresh, 200);
    }

    async function solve() {
      const run = begin("Solving…");
      try {
        const r = await invoke("solve_model", { source: run.source });
        if (!current(run.id)) return;
        showSolved(r);
      } catch (d) {
        showFailure(run.id, d);
      }
    }

    async function check() {
      const run = begin("Checking…");
      try {
        const diags = await invoke("check_model", { source: run.source });
        if (!current(run.id)) return;
        if (!Array.isArray(diags)) { showFailure(run.id, "Check did not return a diagnostic list"); return; }
        if (!diags.length) { setStatus("No unit or structure errors", "ok"); renderHighlight(run.source, null); return; }
        setStatus(diags[0].message, "bad");
        renderHighlight(run.source, diags[0]);
        pushGuide("Heads up", mismatchNote(run.source) + insightForDiag(diags[0].message));
        if (diags.length > 1) {
          out.insertAdjacentHTML("beforeend", diags.map(d => `<div class="diag">${esc(d.message)}</div>`).join(""));
        }
      } catch (d) {
        showFailure(run.id, d);
      }
    }

    async function report() {
      const run = begin("Exporting report…");
      try {
        const saved = await invoke("export_report", { source: run.source });
        if (!current(run.id)) return;
        if (!saved) { setStatus("Report not saved", ""); return; }
        const r = await invoke("solve_model", { source: run.source });
        if (!current(run.id)) return;
        showSolved(r);
        setStatus("Report saved as " + saved.name, "ok");
        out.insertAdjacentHTML("beforeend", `<div class="hash">source sha256: ${esc(saved.source_sha256)}<br>ledger head: ${esc(saved.ledger_head)}</div>`);
      } catch (d) {
        showFailure(run.id, d);
      }
    }

    async function exportPdf() {
      const run = begin("Exporting PDF…");
      try {
        const name = await invoke("export_pdf", { source: run.source });
        if (!current(run.id)) return;
        if (!name) { setStatus("PDF not saved", ""); return; }
        const r = await invoke("solve_model", { source: run.source });
        if (!current(run.id)) return;
        showSolved(r);
        setStatus("PDF saved as " + name, "ok");
      } catch (d) {
        showFailure(run.id, d);
      }
    }

    async function exportSheet() {
      const run = begin("Exporting sheet…");
      try {
        const name = await invoke("export_sheet", { source: run.source });
        if (!current(run.id)) return;
        if (!name) { setStatus("Sheet not saved", ""); return; }
        const r = await invoke("solve_model", { source: run.source });
        if (!current(run.id)) return;
        showSolved(r);
        setStatus("Sheet saved as " + name, "ok");
      } catch (d) {
        showFailure(run.id, d);
      }
    }

    async function importSheet() {
      const range = document.getElementById("sheet-range").value.trim();
      try {
        const next = await invoke("import_sheet", { source: editor.value, range: range || null });
        if (!next) return;
        editor.value = next;
        // The editor reports that write as a keystroke and would check again
        // a moment later, wiping the status this import just settled.
        clearTimeout(refreshTimer);
        refreshDirty();
        renderHighlight(next, null);
        await refresh();
        clearTimeout(refreshTimer);
        if (statusEl.classList.contains("ok")) setStatus("Imported sheet", "ok");
      } catch (d) {
        const run = begin("Importing…");
        showFailure(run.id, d);
      }
    }

    async function verifyReport() {
      try {
        const opened = await invoke("open_report");
        if (!opened) return;
        await invoke("new_model");
        editor.value = opened.source;
        clearTimeout(refreshTimer);
        savedText = "";
        fileNameEl.textContent = opened.name;
        refreshDirty();
        renderHighlight(opened.source, null);
        await refresh();
        clearTimeout(refreshTimer);
        if (statusEl.classList.contains("bad")) return;
        setStatus("Report verified", "ok");
        out.insertAdjacentHTML("beforeend", `<div class="hash">source sha256: ${esc(opened.source_sha256)}<br>ledger head: ${esc(opened.ledger_head)}</div>`);
      } catch (d) {
        const run = begin("Verifying…");
        showFailure(run.id, d);
      }
    }

    document.getElementById("btn-solve").addEventListener("click", solve);
    document.getElementById("btn-check").addEventListener("click", check);
    document.getElementById("btn-report").addEventListener("click", report);
    document.getElementById("btn-pdf").addEventListener("click", exportPdf);
    document.getElementById("btn-verify").addEventListener("click", verifyReport);
    document.getElementById("btn-import").addEventListener("click", importSheet);
    document.getElementById("btn-export-sheet").addEventListener("click", exportSheet);

    // After a pause, check the text and settle it when the check is clean.
    // A refusal stays a sentence. A newer edit drops the older result.
    let refreshTimer = 0;
    async function refresh() {
      const run = begin("Checking…");
      if (!run.source.trim()) { setStatus("Empty model", ""); pushGuide("Start here", insightForEmpty()); return; }
      try {
        const diags = await invoke("check_model", { source: run.source });
        if (!current(run.id)) return;
        if (!Array.isArray(diags)) { showFailure(run.id, "Check did not return a diagnostic list"); return; }
        if (diags.length) {
          setStatus(diags[0].message, "bad");
          renderHighlight(run.source, diags[0]);
          pushGuide("Heads up", mismatchNote(run.source) + insightForDiag(diags[0].message));
          if (diags.length > 1) {
            out.insertAdjacentHTML("beforeend", diags.map(d => `<div class="diag">${esc(d.message)}</div>`).join(""));
          }
          return;
        }
        const r = await invoke("solve_model", { source: run.source });
        if (!current(run.id)) return;
        showSolved(r);
      } catch (d) {
        showFailure(run.id, d);
      }
    }
    // CodeMirror drives edits through this; debounce a check+settle after a pause.
    function onEditorChange() {
      refreshDirty();
      if (!invoke) return;
      clearTimeout(refreshTimer);
      refreshTimer = setTimeout(refresh, 400);
    }

    async function newFile() {
      if (!discardIfDirty()) return;
      clearTimeout(refreshTimer);
      try {
        await invoke("new_model");
      } catch (d) { showFailure(begin("New model").id, d); return; }
      editor.value = "";
      activeTemplate = null;
      markSaved("", "Untitled");
      const run = begin("New model");
      setStatus("New model", "ok");
      pushGuide("Start here", insightForEmpty());
      if (!current(run.id)) return;
    }
    async function openFile() {
      if (!discardIfDirty()) return;
      activeTemplate = null;
      clearTimeout(refreshTimer);
      try {
        const opened = await invoke("open_model");
        if (!opened) return;
        editor.value = opened.source;
        markSaved(opened.source, opened.name);
        await solve();
      } catch (d) { showFailure(begin("Opening…").id, d); }
    }
    async function saveFile() {
      try {
        const name = await invoke("save_model", { source: editor.value });
        if (!name) return;
        markSaved(editor.value, name);
      } catch (d) { showFailure(begin("Saving…").id, d); }
    }

    document.getElementById("btn-new").addEventListener("click", newFile);
    document.getElementById("btn-open").addEventListener("click", openFile);
    document.getElementById("btn-save").addEventListener("click", saveFile);

    async function boot() {
      editor = AvenEditor.mount(editorHost, { doc: "", onChange: onEditorChange, getInvoke: () => invoke });
      if (!invoke) {
        setStatus("Tauri API unavailable — editor preview only", "bad");
        editor.value = "relation Newton {\n    force = mass * acceleration\n}\n\nsolve Newton with {\n    force        = 4200\n    acceleration = 5\n}\n";
        return;
      }
      const templates = await invoke("list_templates");
      tplSel.innerHTML = '<option value="" disabled>Examples ▾</option>'
        + '<option value="blank">Start from scratch</option>'
        + templates.map((t, i) => `<option value="${i}">${esc(t.name)}</option>`).join("");
      tplSel.addEventListener("change", () => {
        const v = tplSel.value;
        if (v === "blank") {
          editor.value = "";
          activeTemplate = null;
          markSaved("", "Untitled");
          clearTimeout(refreshTimer);
          setStatus("Empty model", "");
          pushGuide("Start here", insightForEmpty());
          return;
        }
        const t = templates[Number(v)];
        activeTemplate = { name: t.name, relations: modelRelations(t.source) };
        editor.value = t.source;
        refreshDirty();
        solve();
      });
      const first = templates[0];
      activeTemplate = { name: first.name, relations: modelRelations(first.source) };
      tplSel.value = "0";
      editor.value = first.source;
      markSaved(first.source, "Untitled");
      solve();
    }

    // Live guide: reads whatever the model currently is and says where it
    // stands — not a scripted tour tied to a template. It follows the user's
    // own work, flags when that work has drifted off a loaded example, and
    // collapses to a tab the user can bring back.
    let activeTemplate = null;
    let lastGuide = { kicker: "Guide", text: "" };
    let introShowing = false;
    let introDone = false;
    try { introDone = localStorage.getItem("aven.introDone") === "1"; } catch (_) {}

    function setGuide(kicker, text) {
      document.getElementById("walk-kicker").textContent = kicker || "Guide";
      document.getElementById("walk-text").textContent = text || "";
    }
    // Live updates set lastGuide always, but don't paint over the first-run
    // intro until the user dismisses it.
    function pushGuide(kicker, text) {
      lastGuide = { kicker: kicker, text: text };
      if (!introShowing) setGuide(kicker, text);
    }
    function setCollapsed(c) {
      document.getElementById("walk-body").hidden = c;
      document.getElementById("walk-tab").hidden = !c;
    }
    document.getElementById("walk-hide").addEventListener("click", () => setCollapsed(true));
    document.getElementById("walk-tab").addEventListener("click", () => setCollapsed(false));

    function maybeIntro() {
      if (introDone) { introShowing = false; document.getElementById("walk-intro-actions").hidden = true; return; }
      introShowing = true;
      setGuide("Welcome", "Pick an example at the top right, or just start writing — this guide follows whatever you build. Try a quick calc like 1 + 2 = x, or a relation with a solve block.");
      document.getElementById("walk-intro-actions").hidden = false;
    }
    document.getElementById("walk-next").addEventListener("click", () => {
      introDone = true;
      try { localStorage.setItem("aven.introDone", "1"); } catch (_) {}
      introShowing = false;
      document.getElementById("walk-intro-actions").hidden = true;
      setGuide(lastGuide.kicker, lastGuide.text);
    });

    function modelRelations(src) {
      const names = [];
      const re = /\brelation\s+([A-Za-z_]\w*)/g;
      let m;
      while ((m = re.exec(src))) names.push(m[1]);
      return names;
    }
    // When the user loads an example then edits every one of its relations
    // away, the dropdown label is stale — say so once and follow their model.
    function mismatchNote(src) {
      if (!activeTemplate || !activeTemplate.relations.length) return "";
      const have = new Set(modelRelations(src));
      const gone = activeTemplate.relations.every((n) => !have.has(n));
      if (!gone) return "";
      const name = activeTemplate.name;
      activeTemplate = null;
      tplSel.selectedIndex = 0;
      return "You've moved off the " + name + " example — the guide now follows your own model. ";
    }
    function insightForSolved(r) {
      const steps = (r.explanation || "").trim();
      const bound = r.bound || [];
      const solving = r.solving || [];
      let tail = "";
      if (solving.length && bound.length) {
        tail = "Change a pinned number and this is solved again. Choose Solve for this on a pinned name to make that name the blank.";
      } else if (bound.length) {
        tail = "Change a pinned number and the results are computed again.";
      } else if (steps) {
        tail = "Change a number in the expression and it is computed again.";
      } else {
        tail = "Write a rule, pin the numbers you know, and leave the name you want found out of the solve block.";
      }
      return steps ? steps + "\n" + tail : tail;
    }
    function insightForDiag(msg) {
      const text = (msg || "").trim();
      const m = text.toLowerCase();
      let why = "The marked line stopped the solve.";
      if (m.indexOf("unit") >= 0 || m.indexOf("can't add") >= 0 || m.indexOf("dimensional") >= 0) {
        why = "Those are different kinds of quantity, so Aven stops before producing a number.";
      } else if (m.indexOf("under") >= 0 || m.indexOf("not enough") >= 0 || m.indexOf("determine") >= 0) {
        why = "A name is still free, so the rule does not pick one answer. Pin one more number, or remove a name the rule cannot find.";
      } else if (m.indexOf("inconsist") >= 0 || m.indexOf("over") >= 0 || m.indexOf("conflict") >= 0) {
        why = "A number you pinned is also computed by a rule, and the two disagree. Remove one of them.";
      } else if (m.indexOf("parse") >= 0 || m.indexOf("expect") >= 0 || m.indexOf("syntax") >= 0 || m.indexOf("unexpected") >= 0) {
        why = "That line could not be read as a name, an equals sign, and an expression.";
      }
      return text ? text + "\n" + why : why;
    }
    function insightForEmpty() {
      return "Empty model. Write a relation with a solve block, or a quick calc like 1 + 2 = x, and results appear as you type.";
    }

    maybeIntro();
    boot();
  
