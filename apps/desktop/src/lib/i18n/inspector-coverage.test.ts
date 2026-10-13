import * as ts from "typescript"
import { describe, expect, it } from "vitest"
import { actionTitle, catalogs, dynamicTitle } from "."
import { createRegistry } from "@/lib/actions/registry"

const sources = import.meta.glob<string>([
  "../../features/channel-rack/inspector/*.{ts,tsx}",
  "../../features/mixer/inspector/*.tsx",
  "../../features/playlist/audio/clip-inspector.tsx",
  "../../features/midi/dialogs.tsx",
  "../../features/settings/midi-hardware.tsx",
  "../../features/plugins/*.tsx",
  "../../features/project-info/*.tsx",
  "../../features/updates/section.tsx",
  "!**/*.test.*",
], { query: "?raw", import: "default", eager: true })
const actionSources = import.meta.glob<string>([
  "../../features/**/actions.ts", "../../features/mixer/effect-actions.ts", "../actions/builtin.ts",
], { query: "?raw", import: "default", eager: true })
const display = new Set(["title", "label", "hint", "description", "placeholder", "aria-label", "alt", "help"])
const translation = new Set(["t", "message", "i18nMessage", "localizedMessage", "translatedText", "dynamicTitle"])
function everyCatalog(key: string, location: string) {
  for (const catalog of Object.values(catalogs)) expect(Object.hasOwn(catalog, key), `${location}: missing ${key}`).toBe(true)
}
describe("inspector and dialog localization admission", () => {
  it("rejects raw JSX captions, display attributes, hints, errors and unparameterised dynamic messages", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(25)
    for (const [file, source] of Object.entries(sources)) {
      const syntax = ts.transpileModule(source, { fileName: file, reportDiagnostics: true, compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ESNext } }).diagnostics ?? []
      expect(syntax.filter((item) => item.category === ts.DiagnosticCategory.Error), file).toEqual([])
      const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, file.endsWith("tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS)
      const failures: string[] = []
      const visible = (expression: ts.Expression) => {
        if (ts.isStringLiteral(expression) && /[A-Za-z]/.test(expression.text)) failures.push(expression.getText(tree))
        if (ts.isTemplateExpression(expression) && /[A-Za-z]/.test(expression.head.text + expression.templateSpans.map((span) => span.literal.text).join(""))) failures.push(expression.getText(tree))
        if (ts.isConditionalExpression(expression)) { visible(expression.whenTrue); visible(expression.whenFalse) }
        if (ts.isBinaryExpression(expression) && expression.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken) visible(expression.right)
        if (ts.isBinaryExpression(expression) && expression.operatorToken.kind === ts.SyntaxKind.PlusToken && /[A-Za-z]/.test(expression.getText(tree))) failures.push(expression.getText(tree))
      }
      const walk = (node: ts.Node) => {
        if (ts.isJsxExpression(node) && node.expression && (!ts.isJsxAttribute(node.parent) || display.has(node.parent.name.getText(tree)))) visible(node.expression)
        if (ts.isJsxText(node) && /[A-Za-z]/.test(node.text)) failures.push(node.text.trim())
        if (ts.isJsxAttribute(node) && display.has(node.name.getText(tree)) && node.initializer && ts.isStringLiteral(node.initializer) && /[A-Za-z]/.test(node.initializer.text)) failures.push(node.getText(tree))
        // Data-only option/preset registries carry catalog keys. The mounted
        // Choice/Section/SwitchRow boundaries translate them at render time.
        if (ts.isPropertyAssignment(node) && display.has(node.name.getText(tree)) && ts.isTemplateExpression(node.initializer) && /[A-Za-z]/.test(node.initializer.head.text + node.initializer.templateSpans.map((span) => span.literal.text).join(""))) failures.push(node.getText(tree))
        if (ts.isPropertyAssignment(node) && display.has(node.name.getText(tree)) && ts.isStringLiteral(node.initializer) && /[A-Za-z]{2}/.test(node.initializer.text)) everyCatalog(node.initializer.text,file)
        if (ts.isCallExpression(node)) {
          const name=node.expression.getText(tree)
          const first=node.arguments[0]
          if (translation.has(name) && first && ts.isStringLiteral(first)) everyCatalog(first.text,file)
          if (["useHint","setError","setProgress","toast.success","toast.error"].includes(name) && first && (ts.isTemplateExpression(first) || (ts.isStringLiteral(first) && /[A-Za-z]/.test(first.text)))) failures.push(node.getText(tree))
        }
        if (ts.isJsxAttribute(node) && display.has(node.name.getText(tree)) && node.initializer && ts.isJsxExpression(node.initializer) && node.initializer.expression && ts.isTemplateExpression(node.initializer.expression)) failures.push(node.getText(tree))
        ts.forEachChild(node,walk)
      }
      walk(tree)
      expect(failures, file).toEqual([])
    }
  })
  it("requires registered dynamic palette titles to carry catalog metadata and retranslates without reregistering", () => {
    for (const [file,source] of Object.entries(actionSources)) {
      expect(source.match(/\btitle:\s*`/g),file).toBeNull()
      for (const match of source.matchAll(/dynamicTitle\("([^"\n]+)"/g)) everyCatalog(match[1],file)
    }
    const registry=createRegistry()
    const remove=registry.register([{id:"fixture.dynamic",...dynamicTitle("Fill every {value0} steps",{value0:4}),section:"Edit",run:()=>{}}])
    expect(actionTitle("fixture.dynamic","Fill every 4 steps","es")).toBe("Rellenar cada 4 pasos")
    expect(actionTitle("fixture.dynamic","Fill every 4 steps","en")).toBe("Fill every 4 steps")
    remove()
    expect(actionTitle("fixture.dynamic","Authored project","es")).toBe("Authored project")
  })
})
