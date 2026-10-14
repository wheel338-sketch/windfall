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
  // A line of help about an item, shown beside it or in its tooltip.
  "about",
])
const displayCalls: Readonly<Record<string, readonly number[] | "all">> = {
  useHint: [0], setError: [0], setProgress: [0], setNotice: [0],
  reportError: [0, 1], refuse: [0, 1], attempt: [1],
  askConfirm: "all", askPrompt: "all", askText: "all",
  // window.prompt's second argument is the starting value of the field the user edits, not a caption.
  alert: "all", confirm: "all", prompt: [0],
  toast: [0, 1], "toast.success": [0, 1], "toast.error": [0, 1],
  "toast.warning": [0, 1], "toast.info": [0, 1], "toast.loading": [0, 1],
  "toast.message": [0, 1], "toast.promise": [1],
  setWindowTitle: [0], setTitle: [0],
  fillText: [0], strokeText: [0],
}
const translationExports = new Set([
  "t", "message", "i18nMessage", "localizedMessage", "translatedText", "dynamicTitle",
  "localizeBackendText", "nativeMessage", "actionTitle", "useActionTitle",
  // A history label kept as its key, and the words made from a kept label.
  "historyLabel", "historyLabelText",
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

type GuardContext = { tree: ts.SourceFile; checker: ts.TypeChecker; originalFiles: ReadonlyMap<string, string>; flows: Flows }

type JsxSite = ts.JsxOpeningElement | ts.JsxSelfClosingElement
/**
 * Where values travel in the whole program, gathered once: the calls of every
 * function, parameter and state setter; where a function or setter is handed
 * on as an argument; where a component is drawn; what a `let` is assigned;
 * and what is written into each store with `setState`. The guard follows a
 * value shown on screen back through these to the text it was made from.
 */
type Flows = {
  calls: Map<ts.Symbol, ts.CallExpression[]>
  passes: Map<ts.Symbol, { call: ts.CallExpression; index: number }[]>
  elements: Map<ts.Symbol, JsxSite[]>
  assignments: Map<ts.Symbol, ts.Expression[]>
  storeWrites: Map<ts.Symbol, ts.Expression[]>
}

const unwrap = (node: ts.Expression): ts.Expression =>
  ts.isParenthesizedExpression(node) || ts.isAsExpression(node) || ts.isSatisfiesExpression(node) || ts.isNonNullExpression(node) ? unwrap(node.expression) : node

function resolvedSymbol(checker: ts.TypeChecker, node: ts.Node): ts.Symbol | undefined {
  const symbol = ts.isShorthandPropertyAssignment(node.parent) && node.parent.name === node
    ? checker.getShorthandAssignmentValueSymbol(node.parent) : checker.getSymbolAtLocation(node)
  if (symbol && symbol.flags & ts.SymbolFlags.Alias) {
    const target = checker.getAliasedSymbol(symbol)
    // An import from a module outside the program resolves to nothing; keep the import.
    return target.declarations?.length ? target : symbol
  }
  return symbol
}

/** The module an identifier is imported from, if it is imported. */
function importedFrom(checker: ts.TypeChecker, node: ts.Identifier): string | undefined {
  for (const declaration of checker.getSymbolAtLocation(node)?.getDeclarations() ?? []) {
    if (!ts.isImportSpecifier(declaration)) continue
    const statement = declaration.parent.parent.parent
    if (ts.isImportDeclaration(statement) && ts.isStringLiteral(statement.moduleSpecifier)) return statement.moduleSpecifier.text
  }
  return undefined
}

/** The function that builds a zustand store's state: `create(fn)`, `create<T>()(fn)`, through `persist(fn)` and other middleware. */
function storeCreator(checker: ts.TypeChecker, initializer: ts.Expression): ts.FunctionLikeDeclaration | undefined {
  const call = unwrap(initializer)
  if (!ts.isCallExpression(call)) return undefined
  const callee = ts.isCallExpression(call.expression) ? call.expression.expression : call.expression
  if (!ts.isIdentifier(callee) || callee.text !== "create" || importedFrom(checker, callee) !== "zustand") return undefined
  let state: ts.Expression | undefined = call.arguments[0]
  while (state && ts.isCallExpression(unwrap(state))) state = (unwrap(state) as ts.CallExpression).arguments[0]
  state = state && unwrap(state)
  return state && (ts.isArrowFunction(state) || ts.isFunctionExpression(state)) ? state : undefined
}

/** The store a hook or alias names: `useHintStore`, or `store` after `const store = useHintStore`. */
function storeOf(checker: ts.TypeChecker, expression: ts.Expression, seen = new Set<ts.Symbol>()): ts.Symbol | undefined {
  const value = unwrap(expression)
  if (!ts.isIdentifier(value)) return undefined
  const symbol = resolvedSymbol(checker, value)
  if (!symbol || seen.has(symbol)) return undefined
  seen.add(symbol)
  for (const declaration of symbol.getDeclarations() ?? []) {
    if (!ts.isVariableDeclaration(declaration) || !declaration.initializer) continue
    const initializer = unwrap(declaration.initializer)
    if (ts.isIdentifier(initializer)) return storeOf(checker, initializer, seen)
    if (storeCreator(checker, initializer)) return symbol
  }
  return undefined
}

function gatherFlows(trees: Iterable<ts.SourceFile>, checker: ts.TypeChecker): Flows {
  const flows: Flows = { calls: new Map(), passes: new Map(), elements: new Map(), assignments: new Map(), storeWrites: new Map() }
  const push = <K, V>(map: Map<K, V[]>, key: K | undefined, value: V) => {
    if (key !== undefined) map.set(key, [...(map.get(key) ?? []), value])
  }
  const visit = (node: ts.Node) => {
    if (ts.isCallExpression(node)) {
      const callee = unwrap(node.expression)
      push(flows.calls, resolvedSymbol(checker, ts.isPropertyAccessExpression(callee) ? callee.name : callee), node)
      if (ts.isPropertyAccessExpression(callee) && callee.name.text === "setState" && node.arguments[0])
        push(flows.storeWrites, storeOf(checker, callee.expression), node.arguments[0])
      node.arguments.forEach((argument, index) => {
        const value = unwrap(argument)
        if (ts.isIdentifier(value)) push(flows.passes, resolvedSymbol(checker, value), { call: node, index })
      })
    }
    if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node))
      push(flows.elements, resolvedSymbol(checker, node.tagName), node)
    if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(node.left))
      push(flows.assignments, resolvedSymbol(checker, node.left), node.right)
    ts.forEachChild(node, visit)
  }
  for (const tree of trees) visit(tree)
  return flows
}

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
  const flows = gatherFlows(trees.values(), checker)
  const failures = new Map<string, LocalizationViolation>()
  for (const [path, tree] of trees) {
    const file = originalFiles.get(path) ?? path
    for (const failure of localizationViolations(file, tree.text, hasKey, { tree, checker, originalFiles, flows }))
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
  const { tree, checker, originalFiles, flows } = context
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
      // TRACE-START
      ...((globalThis as { GUARD_TRACE?: boolean }).GUARD_TRACE ? { trace: [...active].map((step) => {
        const at = step.getSourceFile()
        return `${at.fileName}:${at.getLineAndCharacterOfPosition(step.getStart(at)).line + 1} ${step.getText(at).slice(0, 60).replace(/\s+/g, " ")}`
      }) } : {}),
      // TRACE-END
    })
  }
  const literal = (node: ts.Node, text: string, context: string) => {
    const cleaned = text.replace(/\s+/g, " ").trim()
    if (context === "control catalog key" && hasKey(cleaned)) return
    if (cleaned && !invariantReason(cleaned, context)) fail(node, cleaned, context)
  }
  const active = new Set<ts.Node>()
  const argumentBindings: Map<string | ts.Symbol, ts.Expression | readonly ts.Expression[]>[] = []
  const unwrapped = (node: ts.Expression): ts.Expression =>
    ts.isParenthesizedExpression(node) || ts.isAsExpression(node) || ts.isSatisfiesExpression(node) ? unwrapped(node.expression) : node
  /** The object literal an expression is, or the constant's that it names. */
  const objectLiterals = (table: ts.Expression): ts.ObjectLiteralExpression[] => {
    const value = unwrapped(table)
    if (ts.isObjectLiteralExpression(value)) return [value]
    if (!ts.isIdentifier(value)) return []
    return declarationsAt(value).flatMap((declaration) => {
      const initializer = ts.isVariableDeclaration(declaration) && declaration.initializer ? unwrapped(declaration.initializer) : undefined
      return initializer && ts.isObjectLiteralExpression(initializer) ? [initializer] : []
    })
  }
  /** The values of an object literal, or of a constant that is one. */
  const tableValues = (table: ts.Expression): ts.Expression[] | undefined => {
    const literals: ts.ObjectLiteralExpression[] = []
    const value = unwrapped(table)
    if (ts.isObjectLiteralExpression(value)) literals.push(value)
    else if (ts.isIdentifier(value)) for (const declaration of declarationsAt(value)) {
      const initializer = ts.isVariableDeclaration(declaration) && declaration.initializer ? unwrapped(declaration.initializer) : undefined
      if (initializer && ts.isObjectLiteralExpression(initializer)) literals.push(initializer)
    }
    if (!literals.length) return undefined
    return literals.flatMap((literal) => literal.properties.flatMap((property) => ts.isPropertyAssignment(property) ? [property.initializer] : []))
  }
  /** The rows of a literal list, of a constant that is one, or of a table of lists read by key. */
  const rowsOf = (list: ts.Expression, seen = new Set<ts.Node>()): ts.Expression[] | undefined => {
    const value = unwrapped(list)
    if (seen.has(value)) return undefined
    seen.add(value)
    let lists: ts.Expression[] = []
    if (ts.isArrayLiteralExpression(value)) lists = [value]
    else if (ts.isIdentifier(value)) for (const declaration of declarationsAt(value)) {
      if (ts.isVariableDeclaration(declaration) && declaration.initializer && ts.isArrayLiteralExpression(unwrapped(declaration.initializer)))
        lists.push(unwrapped(declaration.initializer))
    } else if (ts.isElementAccessExpression(value)) lists = (tableValues(value.expression) ?? []).map(unwrapped)
    else if (ts.isPropertyAccessExpression(value)) {
      // TABLE.key, a list kept in a table under a name.
      const found = objectLiterals(value.expression).flatMap((table) => table.properties.flatMap((property) =>
        ts.isPropertyAssignment(property) && propertyName(property.name) === value.name.text ? [rowsOf(property.initializer, seen)] : []))
      return found.length && found.every((rows) => rows !== undefined) ? found.flat() as ts.Expression[] : undefined
    }
    if (!lists.length || !lists.every(ts.isArrayLiteralExpression)) return undefined
    return lists.flatMap((literal) => literal.elements.flatMap((element) =>
      // A spread of another literal list contributes its rows; any other spread is kept whole.
      ts.isSpreadElement(element) ? rowsOf(element.expression, seen) ?? [element.expression] : [element]))
  }
  /** What a map callback's item parameter stands for, or undefined when it cannot be told. */
  const mappedItems = (list: ts.Expression, callback: ts.Expression | undefined) => {
    if (!callback || (!ts.isArrowFunction(callback) && !ts.isFunctionExpression(callback))) return undefined
    const item = callback.parameters[0]
    const bindings = new Map<string | ts.Symbol, ts.Expression | readonly ts.Expression[]>()
    if (!item) return bindings
    const symbolOf = (name: ts.Identifier) => checker.getSymbolAtLocation(name) ?? name.text
    // Object.values(TABLE) and Object.entries(TABLE) hand over the table's values.
    const source = unwrapped(list)
    if (ts.isCallExpression(source) && ts.isPropertyAccessExpression(source.expression) &&
      source.expression.expression.getText() === "Object" && source.arguments[0]) {
      const values = tableValues(source.arguments[0])
      if (values && source.expression.name.text === "values" && ts.isIdentifier(item.name)) {
        bindings.set(symbolOf(item.name), values)
        return bindings
      }
      if (values && source.expression.name.text === "entries" && ts.isArrayBindingPattern(item.name)) {
        const value = item.name.elements[1]
        if (value && !ts.isOmittedExpression(value) && ts.isIdentifier(value.name)) bindings.set(symbolOf(value.name), values)
        return bindings
      }
    }
    if (ts.isIdentifier(item.name)) {
      bindings.set(checker.getSymbolAtLocation(item.name) ?? item.name.text, list)
      return bindings
    }
    // A tuple pattern over literal tuples binds each position alone. A row
    // that is not a literal tuple stands for every position.
    if (!ts.isArrayBindingPattern(item.name)) return undefined
    const rows = rowsOf(list)
    if (!rows) return undefined
    const tuples: ts.ArrayLiteralExpression[] = []
    const whole: ts.Expression[] = []
    for (const row of rows.map(unwrapped))
      if (ts.isArrayLiteralExpression(row) && !row.elements.some(ts.isSpreadElement)) tuples.push(row)
      else whole.push(row)
    for (const [position, element] of item.name.elements.entries()) {
      if (ts.isOmittedExpression(element)) continue
      if (!ts.isIdentifier(element.name) || element.dotDotDotToken || element.initializer) return undefined
      bindings.set(checker.getSymbolAtLocation(element.name) ?? element.name.text,
        [...tuples.flatMap((tuple) => tuple.elements[position] ? [tuple.elements[position]] : []), ...whole])
    }
    return bindings
  }
  const declarationsAt = (node: ts.Node): readonly ts.Declaration[] => {
    let symbol = checker.getSymbolAtLocation(node)
    if (symbol && symbol.flags & ts.SymbolFlags.Alias) symbol = checker.getAliasedSymbol(symbol)
    return symbol?.getDeclarations() ?? []
  }
  /** What a function gives back: its expression body, or every `return` that is not a nested function's. */
  const returnedExpressions = (fn: ts.SignatureDeclaration): ts.Expression[] => returnedFrom((fn as ts.FunctionLikeDeclaration).body)
  const returnedFrom = (body: ts.ConciseBody | undefined): ts.Expression[] => {
    if (!body) return []
    if (!ts.isBlock(body)) return [body]
    const found: ts.Expression[] = []
    const returns = (child: ts.Node) => {
      if (ts.isReturnStatement(child) && child.expression) found.push(child.expression)
      if (child !== body && ts.isFunctionLike(child)) return
      ts.forEachChild(child, returns)
    }
    returns(body)
    return found
  }
  const returnsOf = (body: ts.ConciseBody, context: string) => {
    for (const value of returnedFrom(body)) visible(value, context)
  }
  /** A function the value is: an arrow or function expression, or one wrapped in useCallback, memo or forwardRef. */
  const functionValue = (value: ts.Expression | undefined): ts.FunctionLikeDeclaration | undefined => {
    const inner = value && unwrap(value)
    if (!inner) return undefined
    if (ts.isArrowFunction(inner) || ts.isFunctionExpression(inner)) return inner
    if (ts.isCallExpression(inner) && /(?:^|\.)(?:useCallback|memo|forwardRef)$/.test(inner.expression.getText()))
      return functionValue(inner.arguments[0])
    return undefined
  }
  /** The functions a call can run, found through its callee's declarations. */
  const calledFunctions = (call: ts.CallExpression): ts.FunctionLikeDeclaration[] => {
    const callee = unwrap(call.expression)
    return functionsNamed(resolvedSymbol(checker, ts.isPropertyAccessExpression(callee) ? callee.name : callee))
  }
  /**
   * The functions a name can hold: a declared function, one assigned to a
   * variable, a binding's default, or what callers pass for a parameter or a
   * component's prop, such as `stepLabel={(step) => …}`.
   */
  const functionsNamed = (symbol: ts.Symbol | undefined, seen = new Set<ts.Symbol>()): ts.FunctionLikeDeclaration[] => {
    if (!symbol || seen.has(symbol)) return []
    seen.add(symbol)
    const from = (value: ts.Expression | undefined): ts.FunctionLikeDeclaration[] => {
      const fn = functionValue(value)
      if (fn) return [fn]
      const inner = value && unwrap(value)
      return inner && ts.isIdentifier(inner) ? functionsNamed(resolvedSymbol(checker, inner), seen) : []
    }
    const found: ts.FunctionLikeDeclaration[] = []
    for (const declaration of symbol.getDeclarations() ?? []) {
      if ((ts.isFunctionDeclaration(declaration) || ts.isMethodDeclaration(declaration)) && declaration.body) found.push(declaration)
      if ((ts.isVariableDeclaration(declaration) || ts.isPropertyAssignment(declaration)) && declaration.initializer) found.push(...from(declaration.initializer))
      if (ts.isParameter(declaration)) for (const value of argumentsFor(declaration)) found.push(...from(value))
      if (ts.isBindingElement(declaration)) {
        found.push(...from(declaration.initializer))
        const pattern = declaration.parent
        const key = declaration.propertyName ?? declaration.name
        if (ts.isObjectBindingPattern(pattern) && ts.isParameter(pattern.parent) && (ts.isIdentifier(key) || ts.isStringLiteral(key)))
          for (const value of propValues(pattern.parent, key.text)) found.push(...from(value))
      }
    }
    return found
  }
  /** What is written for prop `name` where a component is drawn, or for that property of the object callers pass. */
  const propValues = (parameter: ts.ParameterDeclaration, name: string): ts.Expression[] => {
    const fn = parameter.parent
    const index = fn.parameters.indexOf(parameter)
    const symbol = functionSymbol(fn)
    if (!symbol) return []
    const found: ts.Expression[] = []
    for (const call of callsOf(symbol)) {
      const argument = call.arguments[index] && unwrap(call.arguments[index])
      if (argument && ts.isObjectLiteralExpression(argument)) for (const property of argument.properties)
        if (ts.isPropertyAssignment(property) && propertyName(property.name) === name) found.push(property.initializer)
    }
    if (index === 0) for (const site of flows.elements.get(symbol) ?? []) for (const attribute of site.attributes.properties)
      if (ts.isJsxAttribute(attribute) && attribute.name.getText() === name && attribute.initializer &&
        ts.isJsxExpression(attribute.initializer) && attribute.initializer.expression) found.push(attribute.initializer.expression)
    return found
  }
  /** The symbol callers name a function by: its own name, or the variable or property holding it. */
  const functionSymbol = (fn: ts.SignatureDeclaration): ts.Symbol | undefined => {
    if ((ts.isFunctionDeclaration(fn) || ts.isMethodDeclaration(fn)) && fn.name) return checker.getSymbolAtLocation(fn.name)
    let holder: ts.Node = fn.parent
    while (ts.isParenthesizedExpression(holder) || ts.isAsExpression(holder) ||
      (ts.isCallExpression(holder) && /(?:^|\.)(?:useCallback|memo|forwardRef)$/.test(holder.expression.getText()))) holder = holder.parent
    if ((ts.isVariableDeclaration(holder) || ts.isPropertyAssignment(holder)) && (ts.isIdentifier(holder.name) || ts.isStringLiteral(holder.name)))
      return checker.getSymbolAtLocation(holder.name)
    return undefined
  }
  /** Every call of what `symbol` names, also where it is handed to another function and called there. */
  const callsOf = (symbol: ts.Symbol, seen = new Set<ts.Symbol>()): ts.CallExpression[] => {
    if (seen.has(symbol)) return []
    seen.add(symbol)
    const found = [...(flows.calls.get(symbol) ?? [])]
    for (const { call, index } of flows.passes.get(symbol) ?? []) for (const fn of calledFunctions(call)) {
      const parameter = fn.parameters[index]
      const handed = parameter && ts.isIdentifier(parameter.name) ? checker.getSymbolAtLocation(parameter.name) : undefined
      if (handed) found.push(...callsOf(handed, seen))
    }
    return found
  }
  /** What the callers of a parameter's function pass in its place. */
  const argumentsFor = (parameter: ts.ParameterDeclaration): ts.Expression[] => {
    const fn = parameter.parent
    const index = fn.parameters.indexOf(parameter)
    const symbol = functionSymbol(fn)
    if (!symbol) return []
    return callsOf(symbol).flatMap((call) => parameter.dotDotDotToken ? [...call.arguments.slice(index)] : call.arguments[index] ? [call.arguments[index]] : [])
  }
  /** What callers pass as property `name` of a parameter: an object's property, or a component's prop. */
  const propertyOfParameter = (parameter: ts.ParameterDeclaration, name: string, context: string, seen = new Set<ts.Node>()) => {
    if (seen.has(parameter)) return
    seen.add(parameter)
    const fn = parameter.parent
    const index = fn.parameters.indexOf(parameter)
    const symbol = functionSymbol(fn)
    if (!symbol) return
    for (const call of callsOf(symbol)) if (call.arguments[index]) readProperty(call.arguments[index], name, context, seen)
    if (index !== 0) return
    // A control that translates a prop itself has its keys checked where it is drawn.
    const contract = Object.hasOwn(translatedProps, symbol.name) ? translatedProps[symbol.name] : undefined
    if (contract && fn.getSourceFile().fileName.endsWith(contract.source) && contract.props.includes(name)) return
    for (const site of flows.elements.get(symbol) ?? []) for (const attribute of site.attributes.properties) {
      if (ts.isJsxSpreadAttribute(attribute)) readProperty(attribute.expression, name, context, seen)
      else if (attribute.name.getText() === name && attribute.initializer) {
        if (ts.isStringLiteral(attribute.initializer)) visible(attribute.initializer, context)
        else if (ts.isJsxExpression(attribute.initializer) && attribute.initializer.expression) visible(attribute.initializer.expression, context)
      }
    }
  }
  /** Whatever is written under `field` into a store: its initial state, `set(…)` and `setState(…)` anywhere. */
  const storeField = (store: ts.Symbol, field: string, context: string) => {
    const writes = [...(flows.storeWrites.get(store) ?? [])]
    for (const declaration of store.getDeclarations() ?? []) {
      const creator = ts.isVariableDeclaration(declaration) && declaration.initializer ? storeCreator(checker, declaration.initializer) : undefined
      if (!creator) continue
      writes.push(...returnedExpressions(creator))
      const set = creator.parameters[0]
      const setter = set && ts.isIdentifier(set.name) ? checker.getSymbolAtLocation(set.name) : undefined
      if (setter) for (const call of callsOf(setter)) if (call.arguments[0]) writes.push(call.arguments[0])
    }
    for (const write of writes) {
      // set((state) => ({ … })) writes what its function gives back.
      const fn = functionValue(write)
      for (const value of fn ? returnedExpressions(fn) : [write]) readProperty(value, field, context)
    }
  }
  /** The fields a store selector reads straight off the state: `(state) => state.text`. */
  const selectedFields = (selector: ts.FunctionLikeDeclaration): string[] => {
    const state = selector.parameters[0]
    if (!state) return []
    if (ts.isObjectBindingPattern(state.name)) return state.name.elements.flatMap((element) => {
      const key = element.propertyName ?? element.name
      return ts.isIdentifier(key) ? [key.text] : []
    })
    const symbol = ts.isIdentifier(state.name) ? checker.getSymbolAtLocation(state.name) : undefined
    const fields = new Set<string>()
    const reads = (child: ts.Node) => {
      if (ts.isPropertyAccessExpression(child) && ts.isIdentifier(child.expression) &&
        checker.getSymbolAtLocation(child.expression) === symbol &&
        // state.project.playlist shows the playlist, not the project.
        !((ts.isPropertyAccessExpression(child.parent) || ts.isElementAccessExpression(child.parent)) && child.parent.expression === child))
        fields.add(child.name.text)
      ts.forEachChild(child, reads)
    }
    if (selector.body) reads(selector.body)
    return [...fields]
  }
  /** Follows a binding back to what it was taken from: a destructured property, a prop, or a piece of React state. */
  const bindingValues = (element: ts.BindingElement, context: string) => {
    const pattern = element.parent
    const holder = pattern.parent
    if (ts.isObjectBindingPattern(pattern)) {
      const key = element.propertyName ?? element.name
      if (element.dotDotDotToken || !(ts.isIdentifier(key) || ts.isStringLiteral(key))) return
      if (ts.isVariableDeclaration(holder) && holder.initializer) readProperty(holder.initializer, key.text, context)
      if (ts.isParameter(holder)) propertyOfParameter(holder, key.text, context)
      return
    }
    // const [value, setValue] = useState(initial): the value is the initial one and whatever is set.
    if (!ts.isVariableDeclaration(holder) || !holder.initializer || pattern.elements.indexOf(element) !== 0) return
    const call = unwrap(holder.initializer)
    if (!ts.isCallExpression(call) || !/(?:^|\.)useState$/.test(call.expression.getText())) return
    if (call.arguments[0]) visible(call.arguments[0], context)
    const setter = pattern.elements[1]
    const symbol = setter && !ts.isOmittedExpression(setter) && ts.isIdentifier(setter.name) ? checker.getSymbolAtLocation(setter.name) : undefined
    if (symbol) for (const set of callsOf(symbol)) if (set.arguments[0]) visible(set.arguments[0], context)
  }
  /** Shows property `name` of whatever `source` can be: object literals, list rows, a store's state, a function's result, a parameter. */
  const readProperty = (source: ts.Expression, name: string, context: string, seen = new Set<ts.Node>()) => {
    const value = unwrap(source)
    if (seen.has(value)) return
    seen.add(value)
    const again = (next: ts.Expression) => readProperty(next, name, context, seen)
    if (ts.isObjectLiteralExpression(value)) {
      for (const property of value.properties) {
        if (ts.isSpreadAssignment(property)) { again(property.expression); continue }
        if (!property.name || propertyName(property.name) !== name) continue
        if (ts.isPropertyAssignment(property)) visible(property.initializer, context)
        if (ts.isShorthandPropertyAssignment(property)) visible(property.name, context)
        if (ts.isGetAccessorDeclaration(property) && property.body) returnsOf(property.body, context)
      }
      return
    }
    if (ts.isArrayLiteralExpression(value)) {
      for (const element of value.elements) again(ts.isSpreadElement(element) ? element.expression : element)
      return
    }
    if (ts.isConditionalExpression(value)) { again(value.whenTrue); again(value.whenFalse); return }
    if (ts.isBinaryExpression(value)) {
      const operator = value.operatorToken.kind
      if (operator === ts.SyntaxKind.QuestionQuestionToken || operator === ts.SyntaxKind.BarBarToken) again(value.left)
      if ([ts.SyntaxKind.QuestionQuestionToken, ts.SyntaxKind.BarBarToken, ts.SyntaxKind.AmpersandAmpersandToken].includes(operator)) again(value.right)
      return
    }
    if (ts.isElementAccessExpression(value)) {
      const values = tableValues(value.expression)
      if (values) values.forEach(again)
      else again(value.expression)
      return
    }
    if (ts.isPropertyAccessExpression(value)) {
      for (const declaration of declarationsAt(value.name)) if (ts.isPropertyAssignment(declaration)) again(declaration.initializer)
      return
    }
    if (ts.isIdentifier(value)) {
      for (let index = argumentBindings.length - 1; index >= 0; index -= 1) {
        const bound = argumentBindings[index].get(checker.getSymbolAtLocation(value) ?? value.text)
        if (bound) { for (const item of "kind" in bound ? [bound] : bound) again(item); return }
      }
      const symbol = resolvedSymbol(checker, value)
      for (const declaration of symbol?.getDeclarations() ?? []) {
        if (ts.isVariableDeclaration(declaration)) {
          if (declaration.initializer) again(declaration.initializer)
          for (const assigned of flows.assignments.get(symbol!) ?? []) again(assigned)
        }
        if (ts.isParameter(declaration)) propertyOfParameter(declaration, name, context, seen)
      }
      return
    }
    if (ts.isCallExpression(value)) {
      const callee = unwrap(value.expression)
      // A store's whole state: useStore() or useStore.getState().
      const store = value.arguments.length === 0 ? storeOf(checker, callee) ??
        (ts.isPropertyAccessExpression(callee) && callee.name.text === "getState" ? storeOf(checker, callee.expression) : undefined) : undefined
      if (store) { storeField(store, name, context); return }
      // A row taken from a list.
      if (ts.isPropertyAccessExpression(callee) && ["find", "findLast", "filter", "at", "slice", "sort", "toSorted", "reverse", "toReversed", "concat"].includes(callee.name.text)) {
        again(callee.expression)
        return
      }
      if (/(?:^|\.)useMemo$/.test(callee.getText())) {
        const fn = functionValue(value.arguments[0])
        if (fn) returnedExpressions(fn).forEach(again)
        return
      }
      for (const fn of calledFunctions(value)) {
        const bindings = new Map<string | ts.Symbol, ts.Expression>()
        fn.parameters.forEach((parameter, index) => {
          if (ts.isIdentifier(parameter.name) && value.arguments[index]) bindings.set(checker.getSymbolAtLocation(parameter.name) ?? parameter.name.text, value.arguments[index])
        })
        argumentBindings.push(bindings)
        try { returnedExpressions(fn).forEach(again) } finally { argumentBindings.pop() }
      }
    }
  }
  /** The branch of `typeof x === "string" ? … : …` that only a non-string `x` reaches, when that branch is `x` itself. */
  const narrowedAway = (node: ts.ConditionalExpression): ts.Expression | undefined => {
    const test = unwrapped(node.condition)
    if (!ts.isBinaryExpression(test)) return undefined
    const [query, kind] = ts.isTypeOfExpression(unwrapped(test.left)) ? [unwrapped(test.left), unwrapped(test.right)] : [unwrapped(test.right), unwrapped(test.left)]
    if (!ts.isTypeOfExpression(query) || !ts.isStringLiteral(kind) || kind.text !== "string" || !ts.isIdentifier(unwrapped(query.expression))) return undefined
    const operator = test.operatorToken.kind
    const equal = operator === ts.SyntaxKind.EqualsEqualsEqualsToken || operator === ts.SyntaxKind.EqualsEqualsToken
    if (!equal && operator !== ts.SyntaxKind.ExclamationEqualsEqualsToken && operator !== ts.SyntaxKind.ExclamationEqualsToken) return undefined
    const other = unwrapped(equal ? node.whenFalse : node.whenTrue)
    const subject = checker.getSymbolAtLocation(unwrapped(query.expression))
    return ts.isIdentifier(other) && subject && checker.getSymbolAtLocation(other) === subject ? (equal ? node.whenFalse : node.whenTrue) : undefined
  }
  const translatedFragment = (node: ts.Node, seen = new Set<ts.Node>()): boolean => {
    if (seen.has(node)) return false
    seen.add(node)
    if (ts.isCallExpression(node) && translationName(node.expression)) return true
    if (ts.isIdentifier(node)) for (const declaration of declarationsAt(node))
      if (ts.isVariableDeclaration(declaration) && declaration.initializer && translatedFragment(declaration.initializer, seen)) return true
    return !!ts.forEachChild(node, (child) => translatedFragment(child, seen))
  }
  /** A literal whose declared type is one of a closed set of codes is a value, not a caption. */
  const codeValue = (node: ts.Node): boolean => {
    if (!ts.isStringLiteral(node)) return false
    const type = checker.getContextualType(node)
    if (!type?.isUnion()) return false
    const members = type.types.filter((member) => !(member.flags & (ts.TypeFlags.Null | ts.TypeFlags.Undefined)))
    return members.length > 1 && members.every((member) => member.isStringLiteral())
  }
  // Text that is an operand of the sum. A string passed to a call inside it,
  // as in value("size") + 3, is an argument of arithmetic and not a phrase.
  const proseLiteral = (node: ts.Node): boolean => {
    const prose = (text: string) => !!text.trim() && !invariantReason(text.trim())
    if (ts.isStringLiteralLike(node)) return prose(node.text)
    if (ts.isTemplateExpression(node)) return prose(node.head.text + node.templateSpans.map((span) => span.literal.text).join(""))
    if (ts.isParenthesizedExpression(node)) return proseLiteral(node.expression)
    if (ts.isConditionalExpression(node)) return proseLiteral(node.whenTrue) || proseLiteral(node.whenFalse)
    if (ts.isBinaryExpression(node) && [ts.SyntaxKind.PlusToken, ts.SyntaxKind.QuestionQuestionToken, ts.SyntaxKind.BarBarToken].includes(node.operatorToken.kind))
      return proseLiteral(node.left) || proseLiteral(node.right)
    return false
  }
  const inspect = (node: ts.Node, context: string) => {
    if (ts.isStringLiteralLike(node)) { if (!codeValue(node)) literal(node, node.text, context); return }
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
        if (argument) { for (const value of "kind" in argument ? [argument] : argument) visible(value, context); return }
      }
      const symbol = resolvedSymbol(checker, node)
      const found = (symbol?.getDeclarations() ?? []).filter((declaration) => !ts.isImportSpecifier(declaration))
      if (found.length) for (const declaration of found) {
        // const _exhaustive: never = intent can never hold a value.
        if (ts.isVariableDeclaration(declaration) && declaration.type?.kind === ts.SyntaxKind.NeverKeyword) continue
        if (ts.isVariableDeclaration(declaration)) {
          if (declaration.initializer) visible(declaration.initializer, context)
          // A variable shows whatever is later assigned to it, too.
          for (const value of flows.assignments.get(symbol!) ?? []) visible(value, context)
        }
        if (ts.isBindingElement(declaration)) {
          if (declaration.initializer) visible(declaration.initializer, context)
          bindingValues(declaration, context)
        }
        if (ts.isParameter(declaration)) {
          if (declaration.initializer) visible(declaration.initializer, context)
          // A parameter shows what every caller passes in its place.
          for (const value of argumentsFor(declaration)) visible(value, context)
        }
      }
      else for (const value of declarations.get(node.text) ?? []) visible(value, context)
      return
    }
    if (ts.isPropertyAccessExpression(node)) {
      for (const declaration of declarationsAt(node.name)) {
        if (ts.isPropertyAssignment(declaration)) visible(declaration.initializer, context)
        if (ts.isGetAccessorDeclaration(declaration) && declaration.body) returnsOf(declaration.body, context)
      }
      // What the object it is read from holds under that name.
      readProperty(node.expression, node.name.text, context)
      return
    }
    if (ts.isElementAccessExpression(node)) {
      // A table looked up by key for display shows any of its values, whatever
      // its property names are: LABELS[kind] is a caption as much as a.label.
      const values = tableValues(node.expression)
      if (values) { for (const value of values) visible(value, context); return }
      // Otherwise what is read is shown; the key or index it is read by is not.
      visible(node.expression, context)
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
      // A store hook shows the fields its selector reads.
      const store = storeOf(checker, node.expression)
      if (store) {
        const selector = functionValue(node.arguments[0])
        if (selector) for (const field of selectedFields(selector)) storeField(store, field, context)
        return
      }
      const declared = declarationsAt(ts.isPropertyAccessExpression(node.expression) ? node.expression.name : node.expression)
      const local = calledFunctions(node)
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
      // A mapped list is shown only where its callback shows the item. Binding
      // the item to what it is drawn from finds a caption rendered raw, and
      // leaves a list of technical IDs that is only compared or used as keys.
      const mapped = ["map", "flatMap"].includes(name) && ts.isPropertyAccessExpression(node.expression)
        ? mappedItems(node.expression.expression, node.arguments[0]) : undefined
      if (ts.isPropertyAccessExpression(node.expression) && !mapped) visible(node.expression.expression, context)
      if (["String", "Number"].includes(name)) for (const argument of node.arguments) visible(argument, context)
      // map/render callbacks produce visible text; ordinary call arguments may
      // instead be technical IDs and are inspected by their sink contract.
      if (["map", "flatMap"].includes(name)) {
        if (mapped) argumentBindings.push(mapped)
        try {
          for (const arg of node.arguments) if (ts.isArrowFunction(arg) || ts.isFunctionExpression(arg)) {
            if (ts.isBlock(arg.body)) {
              const returns = (child: ts.Node) => {
                if (ts.isReturnStatement(child) && child.expression) visible(child.expression, context)
                ts.forEachChild(child, returns)
              }
              returns(arg.body)
            } else visible(arg.body, context)
          }
        } finally { if (mapped) argumentBindings.pop() }
      }
      return
    }
    if (ts.isConditionalExpression(node)) {
      // typeof label === "string" ? t(label) : label: no text reaches the other branch.
      const narrowed = narrowedAway(node)
      if (narrowed !== node.whenTrue) visible(node.whenTrue, context)
      if (narrowed !== node.whenFalse) visible(node.whenFalse, context)
      return
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
  // What has been followed already with no call's arguments bound, by context:
  // values now travel far (callers, setters, stores), and many paths meet.
  const followed = new Map<ts.Node, Set<string>>()
  const visible = (node: ts.Node, context: string) => {
    if (active.has(node)) return
    if (argumentBindings.length === 0) {
      const contexts = followed.get(node) ?? new Set<string>()
      if (contexts.has(context)) return
      followed.set(node, contexts.add(context))
    }
    active.add(node)
    try { inspect(node, context) }
    finally { active.delete(node) }
  }
  const propertyName = (name: ts.PropertyName) =>
    ts.isIdentifier(name) || ts.isStringLiteral(name) ? name.text : name.getText()

  const walk = (node: ts.Node) => {
    if (ts.isJsxText(node)) {
      const tag = ts.isJsxElement(node.parent) ? node.parent.openingElement.tagName.getText() : undefined
      // A <code> element holds literal syntax, such as a search expression to type.
      if (tag !== "code") literal(node, node.text, tag === "kbd" ? "keyboard shortcut" : "JSX text")
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
    // A select whose trigger has an empty <SelectValue /> and no `items` to name
    // its values shows the chosen value itself.
    if (ts.isJsxElement(node) && ts.isIdentifier(node.openingElement.tagName) && node.openingElement.tagName.text === "Select" &&
      importedFrom(checker, node.openingElement.tagName)?.endsWith("components/ui/select")) {
      const attributes = node.openingElement.attributes.properties
      const named = (name: string) => attributes.find((attribute): attribute is ts.JsxAttribute => ts.isJsxAttribute(attribute) && attribute.name.getText() === name)
      const bare = (child: ts.Node): boolean => (ts.isJsxSelfClosingElement(child) && child.tagName.getText() === "SelectValue" &&
        !child.attributes.properties.some((attribute) => ts.isJsxAttribute(attribute) && attribute.name.getText() === "children")) ||
        !!ts.forEachChild(child, bare)
      const value = named("value")?.initializer
      if (!named("items") && value && ts.isJsxExpression(value) && value.expression && bare(node)) visible(value.expression, "select value shown as it is")
    }
    // { label } shows whatever the variable or parameter named label holds.
    if (ts.isShorthandPropertyAssignment(node) && displayProperties.has(node.name.text)) visible(node.name, node.name.text)
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
      if (translated && !["actionTitle", "useActionTitle", "nativeMessage", "localizeBackendText", "historyLabelText"].includes(translated)) {
        const keys = (key: ts.Node) => {
          if (ts.isStringLiteralLike(key) && !hasKey(key.text) && !invariantReason(key.text))
            fail(key, key.text, "translation key missing from a shipped catalog")
          if (ts.isConditionalExpression(key)) { keys(key.whenTrue); keys(key.whenFalse) }
        }
        if (node.arguments[0]) keys(node.arguments[0])
      }
      if (translated && ["message", "dynamicTitle", "i18nMessage", "localizedMessage", "historyLabel"].includes(translated)) {
        const values = node.arguments[1]
        if (values && ts.isObjectLiteralExpression(values))
          for (const property of values.properties) {
            // An identifier (`id`, `infoId`) or a file path (`path`, `openPath`) is written as it is.
            if (!property.name || /^(?:id|path)$|(?:Id|Path)$/.test(propertyName(property.name))) continue
            if (ts.isPropertyAssignment(property)) visible(property.initializer, "untranslated semantic message parameter")
            if (ts.isShorthandPropertyAssignment(property)) visible(property.name, "untranslated semantic message parameter")
          }
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
