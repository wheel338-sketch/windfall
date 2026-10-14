import { describe, expect, it } from "vitest"

import { catalogs } from "."
import { invariantKeyboardKeys, invariantLiterals, invariantPatterns, sameAsEnglish } from "./invariants"
import { applicationLocalizationViolations, localizationViolations } from "./source-guard"

const sources = import.meta.glob<string>([
  "../../**/*.{ts,tsx}",
  "!../../bindings/**",
  "!../../test/**",
  "!**/*.test.*", "!**/*.spec.*", "!**/__tests__/**",
  "!./en.ts", "!./es.ts", "!./inspector-messages.ts", "!./native-messages.ts", "!./control-messages.ts",
], { query: "?raw", import: "default", eager: true })
const hasKey = (key: string) => Object.values(catalogs).every((catalog) => Object.hasOwn(catalog, key))

describe("whole application localization admission", () => {
  it("walks all application TypeScript and TSX rather than a scoped feature list", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(900)
    for (const feature of ["effects", "sample-instruments", "modular", "browser", "piano-roll", "playlist", "settings", "help", "channel-rack"])
      expect(Object.keys(sources).some((file) => file.includes(`/features/${feature}/`)), feature).toBe(true)
    const failures = applicationLocalizationViolations(sources, hasKey)
    expect(failures.map(({ file, line, column, text, context }) =>
      `${file}:${line}:${column} [${context}] ${text}`)).toEqual([])
    // Parsing the whole application takes five to six seconds on its own and longer beside other workers.
  }, 60_000)

  it("follows imported captions and helper returns and respects local translation shadowing", () => {
    const failures = applicationLocalizationViolations({
      "copy.ts": 'export const caption = "Imported caption"; export function help() { return "Imported help" }',
      "view.tsx": `
        import { caption, help } from "./copy";
        import { translatedText as t } from "@/lib/i18n";
        function untranslated() { const t = (value: string) => value; return <span>{t("Shadowed caption")}</span> }
        const view = <div>{caption}<p>{help()}</p><span>{t("Mixer")}</span></div>;
      `,
    }, hasKey)
    expect(failures.map(({ text }) => text).sort()).toEqual(["Imported caption", "Imported help", "Shadowed caption"].sort())
    expect(failures.find(({ text }) => text === "Imported caption")?.file).toBe("copy.ts")
  })

  it("detects captions, props, data registries, nested helpers, retained errors and phrases", () => {
    const source = `
      const empty = "Nothing here yet";
      const options = [{ value: "sine", label: "Sine wave" }];
      function help() { return "Choose a source" }
      const title = (count: number) => count > 1 ? "Many channels" : "One channel";
      function Editor() {
        setError("Device failed");
        toast.error("Export failed", { description: "Try another folder" });
        toast.promise(work, { loading: "Loading audio", success: () => "Audio ready", error: () => "Audio failed" });
        registry.register([{id: "technical.id", title: "New command", run() {}}]);
        const action = { whyDisabled() { return "A source is required" } };
        element.textContent = "Canvas status";
        element.setAttribute("title", "Canvas tooltip");
        context.fillText("Canvas caption", 0, 0);
        return <section title={title(2)} aria-label="Editor settings">
          Static caption <span>{empty}</span><p>{help()}</p>
          <ParamControl description="A parameter hint" label={"Cutoff"}/>
          <Tooltip content={<span>More help</span>}/>
          <div>{"Items: " + count}</div><div>{\`Channels: \${count}\`}</div>
        </section>
      }
    `
    const texts = localizationViolations("fixture.tsx", source, hasKey).map(({ text }) => text)
    for (const expected of ["Nothing here yet", "Sine wave", "Choose a source", "Many channels", "One channel", "Device failed", "Export failed", "Try another folder", "Loading audio", "Audio ready", "Audio failed", "New command", "A source is required", "Canvas status", "Canvas tooltip", "Canvas caption", "Editor settings", "Static caption", "A parameter hint", "Cutoff", "More help", "Channels:"])
      expect(texts, expected).toContain(expected)
    expect(texts.some((text) => text.includes('"Items: " + count')), JSON.stringify(texts)).toBe(true)
    expect(texts).not.toContain("technical.id")
    // An unbound tag name must not impersonate a trusted app control.
    expect(texts).toContain("Cutoff")
  })
  it("takes a kept history label for a catalog key and still reports a label written without one", () => {
    expect(localizationViolations("fixture.ts", `
      import { historyLabel, historyLabelText } from "@/lib/i18n";
      const command = { type: "batch", label: historyLabel("Settings"), commands: [] };
      const run = { label: historyLabelText(entry.label) };
    `, hasKey)).toEqual([])
    expect(localizationViolations("fixture.ts", 'import { historyLabel } from "@/lib/i18n"; const command = { label: historyLabel("missing.fixture.key") }', hasKey))
      .toEqual([expect.objectContaining({ text: "missing.fixture.key" })])
    expect(localizationViolations("fixture.ts", 'const command = { type: "batch", label: "Not routed anywhere" }', hasKey))
      .toEqual([expect.objectContaining({ text: "Not routed anywhere" })])
  })

  it("recognizes translated control imports by their defining module, including aliases", () => {
    expect(applicationLocalizationViolations({
      "features/params/param-control.tsx": `
        import { useTranslation } from "@/lib/i18n";
        export function ParamControl({ label }: { label: string }) { const t = useTranslation(); return <span>{t(label)}</span> }
      `,
      "fixture.tsx": `import { ParamControl as Control } from "@/features/params/param-control"; const view = <Control label="Cutoff" />;`,
    }, hasKey)).toEqual([])
  })

  it("accepts translated messages and explicitly justified invariant notation only", () => {
    expect(localizationViolations("fixture.tsx", `
      import { message, translatedText as t } from "@/lib/i18n";
      function Editor() { return <div title={message("Settings")}>
        {t("Mixer")}<span>MIDI</span><span>C♯4</span><span>−∞</span>
      </div> }
    `, hasKey)).toEqual([])
    expect(localizationViolations("fixture.ts", 'import { message } from "@/lib/i18n"; toast.error(message("missing.fixture.key"))', hasKey))
      .toEqual([expect.objectContaining({ text: "missing.fixture.key" })])
    for (const reason of [...Object.values(invariantLiterals), ...invariantPatterns.map(([, reason]) => reason), ...Object.values(sameAsEnglish)])
      expect(reason.trim().length).toBeGreaterThan(10)
    expect(localizationViolations("fixture.tsx", 'function t(value: string) { return value }; const view = <span>{t("Raw caption")}</span>', hasKey))
      .toEqual([expect.objectContaining({ text: "Raw caption" })])
    expect(localizationViolations("fixture.tsx", 'const view = <><span>Space</span><kbd>Space</kbd></>', hasKey))
      .toEqual([expect.objectContaining({ text: "Space", context: "JSX text" })])
    for (const reason of Object.values(invariantKeyboardKeys)) expect(reason.length).toBeGreaterThan(10)
  })
  it("binds a mapped list's item to where its callback shows it", () => {
    const failures = localizationViolations("fixture.tsx", `
      const view = <ul>
        {(["previous", "next"] as const).map((direction) => <li key={direction} data-direction={direction}>{direction === "previous" ? <b/> : <i/>}</li>)}
        {["Shown raw"].map((caption) => <li key={caption}>{caption}</li>)}
        {[["gain", "Tuple caption"]].map(([id, caption]) => <li key={id}>{caption}</li>)}
        {Object.entries(rows).map(([id, caption]) => <li key={id}>{caption}</li>)}
      </ul>;
      const rows = { gain: "Entry caption" };
    `, hasKey)
    // Technical IDs that are only compared or used as keys are not captions.
    expect(failures.map(({ text }) => text).sort()).toEqual(["Entry caption", "Shown raw", "Tuple caption"])
  })
  it("treats every value of a table looked up for display as a caption", () => {
    const failures = localizationViolations("fixture.tsx", `
      const NAMES = { meter: "Peak meter", scope: "Oscilloscope" };
      const view = (kind: "meter" | "scope") => <span title={NAMES[kind]}>{NAMES[kind]}</span>;
    `, hasKey)
    expect(failures.map(({ text }) => text).sort()).toEqual(["Oscilloscope", "Peak meter"])
  })
  it("leaves closed code values and literal syntax samples alone, and nothing else", () => {
    const failures = localizationViolations("fixture.tsx", `
      type Reason = "desktopRequired" | "stale";
      type Status = { reason: Reason | null; label: string };
      const status: Status = { reason: "desktopRequired", label: "Open caption" };
      const view = <p>Type <code>kick OR snare</code> to search</p>;
    `, hasKey)
    expect(failures.map(({ text }) => text).sort()).toEqual(["Open caption", "Type", "to search"])
  })
  it("tells arithmetic from a concatenated phrase", () => {
    const failures = localizationViolations("fixture.tsx", `
      const first = value("preDelayMs") + 3.7 * (0.6 + value("size"));
      const view = <p>{first}<span>{"Delay: " + first}</span></p>;
    `, hasKey)
    expect(failures.map(({ context }) => context)).toContain("concatenated display phrase")
    expect(failures.some(({ text }) => text.includes("3.7"))).toBe(false)
  })
  it("rejects semantic string parameters and concatenations of translated fragments", () => {
    const failures = localizationViolations("fixture.tsx", `
      import { message, translatedText as t } from "@/lib/i18n";
      const title = message("{name} settings", { name: "English parameter name" });
      const view = <span>{\`\${t("Mixer")} \${t("Settings")}\`}</span>;
    `, hasKey)
    expect(failures).toEqual(expect.arrayContaining([
      expect.objectContaining({ text: "English parameter name" }),
      expect.objectContaining({ context: "concatenated translated fragments" }),
    ]))
  })
})
