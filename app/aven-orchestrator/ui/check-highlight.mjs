// Fails if the .aven highlighter drops a keyword, comment, number, string, or unit.
import { startState, tokensOf } from "./aven-mode.mjs"

const sample = [
  "// a comment",
  "/* block */",
  'using "adapter"',
  "signal speed : km/h",
  "relation Travel {",
  "    distance = speed * time",
  "}",
  "solve Travel with {",
  "    speed = 90 km/h",
  "    time = 2.5 h",
  "}",
].join("\n")

const kinds = new Set()
let state = startState()
for (const line of sample.split("\n")) {
  const result = tokensOf(line, state)
  state = result.state
  for (const tok of result.tokens) {
    if (tok.kind) kinds.add(tok.kind)
  }
}

function lineKinds(line) {
  return tokensOf(line).tokens.filter((t) => t.kind)
}

const comment = lineKinds("// a comment")
if (comment.length !== 1 || comment[0].kind !== "comment") {
  throw new Error("line comment not highlighted: " + JSON.stringify(comment))
}

const block = lineKinds("/* block */")
if (!block.every((t) => t.kind === "comment")) {
  throw new Error("block comment not highlighted: " + JSON.stringify(block))
}

const str = lineKinds('using "adapter"')
if (!str.some((t) => t.kind === "keyword" && t.text === "using")) {
  throw new Error("keyword using missing: " + JSON.stringify(str))
}
if (!str.some((t) => t.kind === "string" && t.text.includes("adapter"))) {
  throw new Error("string not highlighted: " + JSON.stringify(str))
}

const unitDecl = lineKinds("signal speed : km/h")
for (const word of ["signal"]) {
  if (!unitDecl.some((t) => t.kind === "keyword" && t.text === word)) {
    throw new Error("keyword missing: " + JSON.stringify(unitDecl))
  }
}
for (const word of ["km", "/", "h"]) {
  if (!unitDecl.some((t) => t.kind === "unit" && t.text === word)) {
    throw new Error("unit " + word + " missing: " + JSON.stringify(unitDecl))
  }
}

const binding = lineKinds("    speed = 90 km/h")
if (!binding.some((t) => t.kind === "number" && t.text === "90")) {
  throw new Error("number missing: " + JSON.stringify(binding))
}
for (const word of ["km", "/", "h"]) {
  if (!binding.some((t) => t.kind === "unit" && t.text === word)) {
    throw new Error("binding unit " + word + " missing: " + JSON.stringify(binding))
  }
}

for (const word of ["relation", "forward", "solve", "signal", "unit", "using"]) {
  const row = lineKinds(word + " name")
  if (!row.some((t) => t.kind === "keyword" && t.text === word)) {
    throw new Error("keyword " + word + " not highlighted")
  }
}

console.log("highlight ok:", [...kinds].sort().join(" "))
