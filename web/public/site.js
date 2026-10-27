// site.js — Shared behaviour for every page: mobile nav, copy buttons, Aven syntax
// highlighting, install-command tabs, workflow tabs, and the docs table of contents.

const KEYWORDS = new Set([
  "relation", "forward", "solve", "with", "signal", "unit", "using", "import",
  "int", "minimize", "maximize", "external", "ipc", "shm", "native",
]);
const FUNCTIONS = new Set([
  "sqrt", "abs", "ceil", "floor", "round", "sign", "ln", "log", "exp", "sin", "cos", "tan",
]);
const BLOCK_KEYWORDS = new Set(["relation", "forward", "solve"]);

const escapeHtml = (s) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** Returns highlighted HTML for one line of Aven source. */
export function highlightLine(line) {
  const out = [];
  const re = /(\/\/.*$)|(\b\d+(?:\.\d+)?(?:e-?\d+)?\b)|([A-Za-z_]\w*)|(>=|<=|==|!=|[-+*/^=<>])|(\s+|.)/g;
  let prevWord = "";
  for (const m of line.matchAll(re)) {
    const [text, com, num, word, op] = m;
    if (com) out.push(`<span class="tok-com">${escapeHtml(com)}</span>`);
    else if (num) out.push(`<span class="tok-num">${num}</span>`);
    else if (word) {
      if (KEYWORDS.has(word)) out.push(`<span class="tok-kw">${word}</span>`);
      else if (FUNCTIONS.has(word)) out.push(`<span class="tok-fn">${word}</span>`);
      else if (BLOCK_KEYWORDS.has(prevWord)) out.push(`<span class="tok-name">${word}</span>`);
      else out.push(word);
      prevWord = word;
    } else if (op) out.push(`<span class="tok-op">${escapeHtml(op)}</span>`);
    else out.push(escapeHtml(text));
  }
  return out.join("");
}

export function highlight(source) {
  return source.split("\n").map(highlightLine).join("\n");
}

const COPY_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/></svg>';

export function makeCopyButton(getText) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "copy-btn";
  btn.innerHTML = `${COPY_ICON}<span>Copy</span>`;
  btn.addEventListener("click", async () => {
    const label = btn.querySelector("span");
    try {
      await navigator.clipboard.writeText(getText());
      label.textContent = "Copied";
    } catch {
      label.textContent = "Press Ctrl+C";
    }
    setTimeout(() => (label.textContent = "Copy"), 1800);
  });
  return btn;
}

function setupNav() {
  const nav = document.querySelector(".nav");
  const toggle = nav?.querySelector(".nav-toggle");
  if (!toggle) return;
  toggle.addEventListener("click", () => {
    const open = nav.dataset.open !== "true";
    nav.dataset.open = String(open);
    toggle.setAttribute("aria-expanded", String(open));
  });
  nav.querySelectorAll(".nav-links a").forEach((a) =>
    a.addEventListener("click", () => {
      nav.dataset.open = "false";
      toggle.setAttribute("aria-expanded", "false");
    })
  );
}

function setupCodeBlocks() {
  document.querySelectorAll('pre[data-lang="aven"]').forEach((pre) => {
    pre.innerHTML = highlight(pre.textContent.replace(/^\n/, ""));
  });
  document.querySelectorAll(".code").forEach((block) => {
    const pre = block.querySelector("pre");
    if (!pre || block.classList.contains("output")) return;
    block.appendChild(makeCopyButton(() => pre.textContent.replace(/^\$ /gm, "")));
  });
}

function setupInstallTabs() {
  const box = document.querySelector(".install");
  if (!box) return;
  const tabs = box.querySelectorAll(".install-tab");
  const cmd = box.querySelector(".install-cmd");
  const note = box.querySelector(".install-note");
  const select = (tab) => {
    tabs.forEach((t) => t.setAttribute("aria-selected", String(t === tab)));
    cmd.textContent = tab.dataset.cmd;
    if (note) note.textContent = tab.dataset.note || "";
  };
  tabs.forEach((t) => t.addEventListener("click", () => select(t)));
  box.querySelector(".install-body").appendChild(makeCopyButton(() => cmd.textContent));
  select(box.querySelector('.install-tab[aria-selected="true"]') || tabs[0]);
}

function setupWorkflow() {
  const steps = document.querySelectorAll("[data-step]");
  const panels = document.querySelectorAll("[data-step-panel]");
  if (!steps.length) return;
  steps.forEach((btn) =>
    btn.addEventListener("click", () => {
      steps.forEach((b) => b.setAttribute("aria-selected", String(b === btn)));
      panels.forEach((p) => (p.hidden = p.dataset.stepPanel !== btn.dataset.step));
      const label = document.querySelector("[data-step-label]");
      if (label) label.textContent = btn.dataset.label;
    })
  );
}

function setupDocsToc() {
  const links = [...document.querySelectorAll(".docs-nav a[href^='#']")];
  if (!links.length) return;
  const sections = links.map((a) => document.querySelector(a.getAttribute("href"))).filter(Boolean);
  const setActive = (id) =>
    links.forEach((a) => a.classList.toggle("active", a.getAttribute("href") === `#${id}`));
  const observer = new IntersectionObserver(
    (entries) => {
      const visible = entries.filter((e) => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
      if (visible[0]) setActive(visible[0].target.id);
    },
    { rootMargin: "-80px 0px -65% 0px" }
  );
  sections.forEach((s) => observer.observe(s));

  const jump = document.querySelector(".docs-jump select");
  jump?.addEventListener("change", () => {
    document.querySelector(jump.value)?.scrollIntoView();
    history.replaceState(null, "", jump.value);
  });
}

setupNav();
setupCodeBlocks();
setupInstallTabs();
setupWorkflow();
setupDocsToc();
