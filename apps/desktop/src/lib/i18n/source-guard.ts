import * as ts from "typescript"

import { invariantReason } from "./invariants"

// Includes the app's ContextItem, ParamGroup/Control, prompt/Choice, Field,
// panel, tooltip, alert, Empty and cmdk component contracts, as well as HTML.
const displayProperties = new Set([
  "title", "label", "placeholder", "aria-label", "aria-description", "aria-valuetext",
  "alt", "description", "hint", "help", "tooltip", "heading", "caption", "legend",
  "submenu", "reason", "emptyText", "loadingText", "errorText", "onText", "offText",
  "message", "notice", "confirmation", "cancelText", "confirmText",
  "submitLabel", "cancelLabel", "clipLabel", "whyDisabled",
])
const displayCalls: Readonly<Record<string, readonly number[] | "all">> = {
  useHint: [0], setError: [0], setProgress: [0], setNotice: [0],
  reportError: [0, 1], refuse: [0, 1], attempt: [1],
  askConfirm: "all", askPrompt: "all", askText: "all",
  alert: "all", confirm: "all", prompt: "all",
  toast: [0, 1], "toast.success": [0, 1], "toast.error": [0, 1],
  "toast.warning": [0, 1], "toast.info": [0, 1], "toast.loading": [0, 1],
  "toast.message": [0, 1], "toast.promise": [1],
  setWindowTitle: [0], setTitle: [0],
  fillText: [0], strokeText: [0],
}
const translationExports = new Set([
  "t", "message", "i18nMessage", "localizedMessage", "translatedText", "dynamicTitle",
  "localizeBackendText", "nativeMessage", "actionTitle", "useActionTitle",
])
// These controls translate canonical catalog keys themselves, and subscribe to
// the locale. The scanner still verifies every literal key at that boundary.
const translatedProps: Readonly<Record<string, { source: string; props: readonly string[] }>> = {
  ParamGroup: { source: "/features/params/param-group.tsx", props: ["title"] },
  ParamControl: { source: "/features/params/param-control.tsx", props: ["label", "description"] },
  ParamFader: { source: "/features/effects/controls/param-fader.tsx", props: ["label", "description"] },
}

export type LocalizationViolation = {
  file: string
  line: number
  column: number
  text: string
  context: string
}

type GuardContext = { tree: ts.SourceFile; checker: ts.TypeChecker; originalFiles: ReadonlyMap<string, string> }

function normalizedPath(path: string): string {
  const parts: string[] = []
  for (const part of path.replaceAll("\\", "/").split("/")) {
    if (!part || part === ".") continue
    if (part === "..") parts.pop()
    else parts.push(part)
  }
  return `/${parts.join("/")}`
}

/** One virtual program gives the guard lexical scopes and cross-module symbols.
 * It reads only supplied source strings and never emits or loads dependencies. */
export function applicationLocalizationViolations(sources: Readonly<Record<string, string>>, hasKey: (key: string) => boolean): LocalizationViolation[] {
  const trees = new Map<string, ts.SourceFile>()
  const originalFiles = new Map<string, string>()
  for (const [file, source] of Object.entries(sources)) {
    const path = normalizedPath(file.startsWith(".") ? `/src/lib/i18n/${file}` : `/src/${file}`)
    trees.set(path, ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true,
      file.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS))
    originalFiles.set(path, file)
  }
  const host: ts.CompilerHost = {
    getSourceFile: (path) => trees.get(normalizedPath(path)),
    getDefaultLibFileName: () => "/none.d.ts",
    writeFile: () => {}, getCurrentDirectory: () => "/src",
    getDirectories: () => [], fileExists: (path) => trees.has(normalizedPath(path)),
    readFile: (path) => trees.get(normalizedPath(path))?.text,
    getCanonicalFileName: (path) => normalizedPath(path),
    useCaseSensitiveFileNames: () => true, getNewLine: () => "\n",
    resolveModuleNames: (modules, containingFile) => modules.map((module) => {
      const prefix = containingFile.slice(0, containingFile.lastIndexOf("/"))
      const path = normalizedPath(module.startsWith("@/") ? `/src/${module.slice(2)}` : `${prefix}/${module}`)
      for (const suffix of ["", ".ts", ".tsx", "/index.ts", "/index.tsx"]) {
        const found = `${path}${suffix}`
        if (trees.has(found)) return { resolvedFileName: found, extension: found.endsWith(".tsx") ? ts.Extension.Tsx : ts.Extension.Ts }
      }
      return undefined
    }),
  }
  const program = ts.createProgram([...trees.keys()], {
    noLib: true, noEmit: true, noResolve: true,
    target: ts.ScriptTarget.Latest, jsx: ts.JsxEmit.Preserve,
    module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.Bundler,
  }, host)
  const checker = program.getTypeChecker()
  const failures = new Map<string, LocalizationViolation>()
  for (const [path, tree] of trees) {
    const file = originalFiles.get(path) ?? path
    for (const failure of localizationViolations(file, tree.text, hasKey, { tree, checker, originalFiles }))
      failures.set(`${failure.file}:${failure.line}:${failure.column}:${failure.text}`, failure)
  }
  return [...failures.values()]
}

/**
 * Source-only admission, using TS/TSX syntax rather than regexes. Display data
 * properties are checked even in .ts registries, so passing an object/array to
 * a menu or a custom control cannot conceal a literal. Within a source file,
 * display expressions follow local constants and function return values.
 * Catalog existence is checked independently for every literal translation key.
 * Runtime backend/descriptors need their separate exhaustive catalog tests.
 */
export function localizationViolations(
  file: string,
  source: string,
  hasKey: (key: string) => boolean,
  context?: GuardContext
): LocalizationViolation[] {
  if (!context) return applicationLocalizationViolations({ [file]: source }, hasKey)
  const { tree, checker, originalFiles } = context
  const violations = new Map<string, LocalizationViolation>()
  const declarations = new Map<string, ts.Expression[]>()
  const functions = new Map<string, ts.FunctionLikeDeclaration[]>()
  const translations = new Map<string, string>()
  const factories = new Set<string>()
  const namespaces = new Set<string>()
  const translationSymbols = new Map<ts.Symbol, string>()
  const namespaceSymbols = new Set<ts.Symbol>()
  const factorySymbols = new Set<ts.Symbol>()
  const localizationModule = (module: string, owner: ts.SourceFile) =>
    module === "@/lib/i18n" || /(?:^|\/)i18n(?:\/index)?$/.test(module) ||
    ((module === "." || module === "./index") && owner.fileName.includes("/lib/i18n/"))
  const importedName = (symbol: ts.Symbol | undefined): string | undefined => {
    for (const declaration of symbol?.getDeclarations() ?? []) {
      if (!ts.isImportSpecifier(declaration)) continue
      const statement = declaration.parent.parent.parent
      if (ts.isImportDeclaration(statement) && ts.isStringLiteral(statement.moduleSpecifier) &&
        localizationModule(statement.moduleSpecifier.text, declaration.getSourceFile()))
        return declaration.propertyName?.text ?? declaration.name.text
    }
    return undefined
  }
  const remember = (node: ts.Identifier, original: string) => {
    const symbol = checker.getSymbolAtLocation(node)
    if (symbol) translationSymbols.set(symbol, original)
  }
  // Recognize actual imports and factory-produced translators, not any local
  // function that happens to be called t/message. This also handles aliases.
  for (const node of tree.statements) {
    if (!ts.isImportDeclaration(node) || !ts.isStringLiteral(node.moduleSpecifier)) continue
    const module = node.moduleSpecifier.text
    if (module !== "@/lib/i18n" && !/(?:^|\/)i18n(?:\/index)?$/.test(module) &&
      !((module === "." || module === "./index") && file.includes("/i18n/"))) continue
    const bindings = node.importClause?.namedBindings
    if (bindings && ts.isNamespaceImport(bindings)) {
      namespaces.add(bindings.name.text)
      const symbol = checker.getSymbolAtLocation(bindings.name)
      if (symbol) namespaceSymbols.add(symbol)
    }
    if (bindings && ts.isNamedImports(bindings)) for (const binding of bindings.elements) {
      const original = binding.propertyName?.text ?? binding.name.text
      if (translationExports.has(original)) { translations.set(binding.name.text, original); remember(binding.name, original) }
      if (original === "useTranslation") {
        factories.add(binding.name.text)
        const symbol = checker.getSymbolAtLocation(binding.name)
        if (symbol) factorySymbols.add(symbol)
      }
    }
  }
  if (/(?:^|\/)lib\/i18n\/index\.ts$/.test(tree.fileName))
    for (const node of tree.statements) if (ts.isFunctionDeclaration(node) && node.name && translationExports.has(node.name.text))
      remember(node.name, node.name.text)
  const translationName = (expression: ts.Expression): string | undefined => {
    if (ts.isIdentifier(expression)) {
      const symbol = checker.getSymbolAtLocation(expression)
      if (!symbol) return translations.get(expression.text)
      const remembered = translationSymbols.get(symbol)
      if (remembered) return remembered
      const imported = importedName(symbol)
      if (imported && translationExports.has(imported)) return imported
      for (const declaration of symbol.getDeclarations() ?? []) {
        if (ts.isFunctionDeclaration(declaration) && declaration.name &&
          /(?:^|\/)lib\/i18n\/index\.ts$/.test(declaration.getSourceFile().fileName) &&
          translationExports.has(declaration.name.text)) return declaration.name.text
        if (ts.isVariableDeclaration(declaration) && declaration.initializer && ts.isCallExpression(declaration.initializer) &&
          importedName(checker.getSymbolAtLocation(declaration.initializer.expression)) === "useTranslation") return "t"
      }
      return undefined
    }
    if (ts.isPropertyAccessExpression(expression) && ts.isIdentifier(expression.expression) &&
      translationExports.has(expression.name.text)) {
      const symbol = checker.getSymbolAtLocation(expression.expression)
      if (symbol ? namespaceSymbols.has(symbol) : namespaces.has(expression.expression.text)) return expression.name.text
      for (const declaration of symbol?.getDeclarations() ?? []) if (ts.isNamespaceImport(declaration)) {
        const statement = declaration.parent.parent
        if (ts.isImportDeclaration(statement) && ts.isStringLiteral(statement.moduleSpecifier) &&
          localizationModule(statement.moduleSpecifier.text, declaration.getSourceFile())) return expression.name.text
      }
    }
    return undefined
  }
  const add = (name: string, expression: ts.Expression) =>
    declarations.set(name, [...(declarations.get(name) ?? []), expression])
  const index = (node: ts.Node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer &&
      ts.isCallExpression(node.initializer) && ts.isIdentifier(node.initializer.expression) &&
      factories.has(node.initializer.expression.text)) {
      const symbol = checker.getSymbolAtLocation(node.initializer.expression)
      if (!symbol || factorySymbols.has(symbol)) { translations.set(node.name.text, "t"); remember(node.name, "t") }
    }
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer)
      add(node.name.text, node.initializer)
    if (ts.isFunctionDeclaration(node) && node.name)
      functions.set(node.name.text, [...(functions.get(node.name.text) ?? []), node])
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer &&
      (ts.isArrowFunction(node.initializer) || ts.isFunctionExpression(node.initializer)))
      functions.set(node.name.text, [...(functions.get(node.name.text) ?? []), node.initializer])
    ts.forEachChild(node, index)
  }
  index(tree)

  const fail = (node: ts.Node, text: string, context: string) => {
    const owner = node.getSourceFile()
    const location = owner.getLineAndCharacterOfPosition(node.getStart(owner))
    const filename = originalFiles.get(owner.fileName) ?? file
    // Imported helper failures belong to their source location.
    const identity = `${filename}:${node.getStart(owner)}:${node.getEnd()}`
    violations.set(identity, {
      file: filename, line: location.line + 1, column: location.character + 1, text, context,
    })
  }
  const literal = (node: ts.Node, text: string, context: string) => {
    const cleaned = text.replace(/\s+/g, " ").trim()
    if (context === "control catalog key" && hasKey(cleaned)) return
    if (cleaned && !invariantReason(cleaned, context)) fail(node, cleaned, context)
  }
  const active = new Set<ts.Node>()
  const argumentBindings: Map<string | ts.Symbol, ts.Expression>[] = []
  const declarationsAt = (node: ts.Node): readonly ts.Declaration[] => {
    let symbol = checker.getSymbolAtLocation(node)
    if (symbol && symbol.flags & ts.SymbolFlags.Alias) symbol = checker.getAliasedSymbol(symbol)
    return symbol?.getDeclarations() ?? []
  }
  const translatedFragment = (node: ts.Node, seen = new Set<ts.Node>()): boolean => {
    if (seen.has(node)) return false
    seen.add(node)
    if (ts.isCallExpression(node) && translationName(node.expression)) return true
    if (ts.isIdentifier(node)) for (const declaration of declarationsAt(node))
      if (ts.isVariableDeclaration(declaration) && declaration.initializer && translatedFragment(declaration.initializer, seen)) return true
    return !!ts.forEachChild(node, (child) => translatedFragment(child, seen))
  }
  const proseLiteral = (node: ts.Node): boolean =>
    (ts.isStringLiteralLike(node) && !!node.text.trim() && !invariantReason(node.text.trim())) ||
    !!ts.forEachChild(node, proseLiteral)
  const inspect = (node: ts.Node, context: string) => {
    if (ts.isStringLiteralLike(node)) { literal(node, node.text, context); return }
    if (ts.isTemplateExpression(node)) {
      // A semantic phrase needs one parameterized key, not translated pieces.
      const text = node.head.text + node.templateSpans.map((span) => span.literal.text).join("")
      if (context === "control catalog key" && text.trim() && !invariantReason(text.trim()))
        fail(node, node.getText(), "unparameterized control label")
      else literal(node, text, context)
      for (const span of node.templateSpans)
        if (translatedFragment(span.expression)) fail(node, node.getText(), "concatenated translated fragments")
      for (const span of node.templateSpans) visible(span.expression, context)
      return
    }
    if (ts.isIdentifier(node)) {
      for (let index = argumentBindings.length - 1; index >= 0; index -= 1) {
        const argument = argumentBindings[index].get(checker.getSymbolAtLocation(node) ?? node.text)
        if (argument) { visible(argument, context); return }
      }
      const found = declarationsAt(node)
      if (found.length) for (const declaration of found) {
        if (ts.isVariableDeclaration(declaration) && declaration.initializer) visible(declaration.initializer, context)
        if (ts.isBindingElement(declaration) && declaration.initializer) visible(declaration.initializer, context)
      }
      else for (const value of declarations.get(node.text) ?? []) visible(value, context)
      return
    }
    if (ts.isPropertyAccessExpression(node)) {
      for (const declaration of declarationsAt(node.name))
        if (ts.isPropertyAssignment(declaration)) visible(declaration.initializer, context)
      return
    }
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node) || ts.isJsxFragment(node)) {
      walk(node)
      return
    }
    if (ts.isArrowFunction(node) || ts.isFunctionExpression(node)) {
      if (ts.isBlock(node.body)) {
        const returns = (child: ts.Node) => {
          if (ts.isReturnStatement(child) && child.expression) visible(child.expression, context)
          if (child !== node.body && ts.isFunctionLike(child)) return
          ts.forEachChild(child, returns)
        }
        returns(node.body)
      } else visible(node.body, context)
      return
    }
    if (ts.isCallExpression(node)) {
      const full = node.expression.getText()
      const name = ts.isPropertyAccessExpression(node.expression) ? node.expression.name.text : full
      if (translationName(node.expression)) return
      const declared = declarationsAt(ts.isPropertyAccessExpression(node.expression) ? node.expression.name : node.expression)
      const local: ts.FunctionLikeDeclaration[] = []
      for (const declaration of declared) {
        if (ts.isFunctionDeclaration(declaration) || ts.isMethodDeclaration(declaration)) local.push(declaration)
        if (ts.isVariableDeclaration(declaration) && declaration.initializer &&
          (ts.isArrowFunction(declaration.initializer) || ts.isFunctionExpression(declaration.initializer))) local.push(declaration.initializer)
      }
      for (const fn of declared.length ? local : functions.get(name) ?? []) {
        const bindings = new Map<string | ts.Symbol, ts.Expression>()
        fn.parameters.forEach((parameter, index) => {
          if (ts.isIdentifier(parameter.name) && node.arguments[index]) bindings.set(checker.getSymbolAtLocation(parameter.name) ?? parameter.name.text, node.arguments[index])
        })
        const returns = (child: ts.Node) => {
          if (ts.isReturnStatement(child) && child.expression) visible(child.expression, context)
          // A nested callback's return is not the enclosing helper's return.
          if (child !== fn && ts.isFunctionLike(child)) return
          ts.forEachChild(child, returns)
        }
        argumentBindings.push(bindings)
        try {
          if (ts.isArrowFunction(fn) && !ts.isBlock(fn.body)) visible(fn.body, context)
          else if (fn.body) returns(fn.body)
        } finally { argumentBindings.pop() }
      }
      if (ts.isPropertyAccessExpression(node.expression)) visible(node.expression.expression, context)
      if (["String", "Number"].includes(name)) for (const argument of node.arguments) visible(argument, context)
      // map/render callbacks produce visible text; ordinary call arguments may
      // instead be technical IDs and are inspected by their sink contract.
      if (["map", "flatMap"].includes(name)) {
        for (const arg of node.arguments) if (ts.isArrowFunction(arg) || ts.isFunctionExpression(arg)) {
          if (ts.isBlock(arg.body)) {
            const returns = (child: ts.Node) => {
              if (ts.isReturnStatement(child) && child.expression) visible(child.expression, context)
              ts.forEachChild(child, returns)
            }
            returns(arg.body)
          } else visible(arg.body, context)
        }
      }
      return
    }
    if (ts.isConditionalExpression(node)) {
      visible(node.whenTrue, context); visible(node.whenFalse, context); return
    }
    if (ts.isBinaryExpression(node)) {
      if (node.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken) {
        visible(node.right, context); return
      }
      if (node.operatorToken.kind === ts.SyntaxKind.PlusToken &&
        (proseLiteral(node) || translatedFragment(node))) fail(node, node.getText(), "concatenated display phrase")
      visible(node.left, context); visible(node.right, context); return
    }
    if (ts.isObjectLiteralExpression(node)) {
      for (const prop of node.properties) if (ts.isPropertyAssignment(prop) &&
        displayProperties.has(propertyName(prop.name))) visible(prop.initializer, context)
      return
    }
    ts.forEachChild(node, (child) => visible(child, context))
  }
  const visible = (node: ts.Node, context: string) => {
    if (active.has(node)) return
    active.add(node)
    try { inspect(node, context) }
    finally { active.delete(node) }
  }
  const propertyName = (name: ts.PropertyName) =>
    ts.isIdentifier(name) || ts.isStringLiteral(name) ? name.text : name.getText()

  const walk = (node: ts.Node) => {
    if (ts.isJsxText(node)) {
      const keycap = ts.isJsxElement(node.parent) && node.parent.openingElement.tagName.getText() === "kbd"
      literal(node, node.text, keycap ? "keyboard shortcut" : "JSX text")
    }
    if (ts.isJsxExpression(node) && node.expression && !ts.isJsxAttribute(node.parent))
      visible(node.expression, "JSX expression")
    if (ts.isJsxAttribute(node) && displayProperties.has(node.name.getText()) && node.initializer) {
      const opening = node.parent.parent
      const tag = ts.isJsxOpeningElement(opening) || ts.isJsxSelfClosingElement(opening) ? opening.tagName : undefined
      const translated = tag ? declarationsAt(tag).find((declaration) => {
        if (!ts.isFunctionDeclaration(declaration) && !ts.isVariableDeclaration(declaration)) return false
        if (!declaration.name || !ts.isIdentifier(declaration.name)) return false
        const contract = translatedProps[declaration.name.text]
        return contract && declaration.getSourceFile().fileName.endsWith(contract.source) && contract.props.includes(node.name.getText())
      }) : undefined
      const direct = ts.isStringLiteral(node.initializer) ? node.initializer :
        ts.isJsxExpression(node.initializer) && node.initializer.expression && ts.isStringLiteralLike(node.initializer.expression) ? node.initializer.expression : undefined
      if (direct && translated) {
        if (!hasKey(direct.text) && !invariantReason(direct.text))
          fail(direct, direct.text, "control label missing from a shipped catalog")
        return
      }
      if (ts.isJsxExpression(node.initializer)) {
        if (node.initializer.expression) visible(node.initializer.expression, translated ? "control catalog key" : node.name.getText())
      } else if (ts.isStringLiteral(node.initializer)) literal(node.initializer, node.initializer.text, node.name.getText())
    }
    if (ts.isPropertyAssignment(node) && displayProperties.has(propertyName(node.name))) {
      visible(node.initializer, propertyName(node.name))
    }
    if (ts.isBindingElement(node) && ts.isIdentifier(node.name) &&
      displayProperties.has(node.name.text) && node.initializer) visible(node.initializer, "display prop default")
    if (ts.isParameter(node) && ts.isIdentifier(node.name) &&
      displayProperties.has(node.name.text) && node.initializer) visible(node.initializer, "display parameter default")
    if (ts.isMethodDeclaration(node) && displayProperties.has(propertyName(node.name)) && node.body) {
      const returns = (child: ts.Node) => {
        if (ts.isReturnStatement(child) && child.expression) visible(child.expression, "display helper return")
        if (child !== node.body && ts.isFunctionLike(child)) return
        ts.forEachChild(child, returns)
      }
      returns(node.body)
    }
    if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken &&
      ts.isPropertyAccessExpression(node.left) &&
      (displayProperties.has(node.left.name.text) || ["textContent", "innerText"].includes(node.left.name.text)))
      visible(node.right, "imperative display assignment")
    if (ts.isCallExpression(node)) {
      const full = node.expression.getText()
      const name = ts.isPropertyAccessExpression(node.expression) ? node.expression.name.text : full
      const positions = Object.hasOwn(displayCalls, full) ? displayCalls[full] : Object.hasOwn(displayCalls, name) ? displayCalls[name] : undefined
      if (positions) node.arguments.forEach((arg, index) => {
        if (positions === "all" || positions.includes(index)) visible(arg, full)
      })
      if (full === "toast.promise" && node.arguments[1] && ts.isObjectLiteralExpression(node.arguments[1]))
        for (const property of node.arguments[1].properties) if (ts.isPropertyAssignment(property) &&
          ["loading", "success", "error", "description"].includes(propertyName(property.name)))
          visible(property.initializer, "promise toast message")
      if (name === "setAttribute" && node.arguments[0] && ts.isStringLiteralLike(node.arguments[0]) &&
        displayProperties.has(node.arguments[0].text) && node.arguments[1]) visible(node.arguments[1], "imperative display attribute")
      const translated = translationName(node.expression)
      if (translated && !["actionTitle", "useActionTitle", "nativeMessage", "localizeBackendText"].includes(translated)) {
        const keys = (key: ts.Node) => {
          if (ts.isStringLiteralLike(key) && !hasKey(key.text) && !invariantReason(key.text))
            fail(key, key.text, "translation key missing from a shipped catalog")
          if (ts.isConditionalExpression(key)) { keys(key.whenTrue); keys(key.whenFalse) }
        }
        if (node.arguments[0]) keys(node.arguments[0])
      }
      if (translated && ["message", "dynamicTitle", "i18nMessage", "localizedMessage"].includes(translated)) {
        const values = node.arguments[1]
        if (values && ts.isObjectLiteralExpression(values))
          for (const property of values.properties) if (ts.isPropertyAssignment(property))
            visible(property.initializer, "untranslated semantic message parameter")
      }
    }
    // App-created errors can reach reportError/the error boundary later.
    if (ts.isNewExpression(node) && node.expression.getText() === "Error" && node.arguments?.[0])
      visible(node.arguments[0], "Error diagnostic")
    ts.forEachChild(node, walk)
  }
  walk(tree)
  return [...violations.values()].sort((a, b) => a.line - b.line || a.column - b.column)
}
