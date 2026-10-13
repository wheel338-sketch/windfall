import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { ContextActions } from "@/components/context-actions"
import { TooltipProvider } from "@/components/ui/tooltip"
import { EmptyProject } from "@/features/layout/empty-states"
import { CommandPalette, searchActions } from "@/features/palette/command-palette"
import { GenericParamEditor, instrumentDescriptor } from "@/features/params"
import { SettingsDialog } from "@/features/settings/settings-dialog"
import { installKeymap, registry } from "@/lib/actions"
import { errorMessage } from "@/lib/ipc/backend"
import { useUiStore } from "@/lib/store/ui"
import { startTestApp } from "@/test/harness"

import { catalogs, dynamicTitle, localizeBackendText, message, setLanguage, translatedText } from "."
import { LanguageBoundary } from "./boundary"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let stop: () => void
let uninstall: () => void
let unregister: () => void

beforeEach(async () => {
  ;({ stop } = await startTestApp())
  uninstall = installKeymap()
  // A command that cannot run, to show why where its shortcut would be.
  unregister = registry.register([{
    id: "test.language.samplersOnly",
    ...dynamicTitle("Replace sample from an audio file…", {}),
    section: "Channels",
    enabled: () => false,
    whyDisabled: () => translatedText("Samplers only"),
    run() {},
  }])
})
afterEach(() => {
  act(() => setLanguage("en"))
  unregister()
  uninstall()
  stop()
})

/** A failure the app kept as it was raised: English text that is a catalog entry. */
function RetainedFailure({ source }: { source: string }) {
  return <p role="alert">{localizeBackendText(source)}</p>
}

const fourOp = instrumentDescriptor("fourOp")

function Surfaces() {
  return (
    <TooltipProvider>
      <LanguageBoundary>
        <ContextActions items={["edit.undo", { ...dynamicTitle("Remove effect", {}), run() {} }, "test.language.samplersOnly"]}>
          <div data-testid="area">Area</div>
        </ContextActions>
        <EmptyProject />
        <section data-testid="editor">
          <GenericParamEditor descriptor={fourOp} params={fourOp.defaults} setParam={() => {}} />
        </section>
        <RetainedFailure source="This library selection is out of date. Refresh the library and select the file again." />
      </LanguageBoundary>
      <CommandPalette />
      <SettingsDialog />
    </TooltipProvider>
  )
}

const headings = () => within(screen.getByTestId("editor")).getAllByRole("heading").map((heading) => heading.textContent ?? "")

describe("changing the language while the app is open", () => {
  it("retranslates a menu, a dialog, an instrument editor, a kept failure, the palette and a disabled reason", async () => {
    const user = userEvent.setup()
    render(<Surfaces />)

    // English first, so the change is what is measured.
    expect(screen.getByText("This project has no sounds yet")).toBeVisible()
    expect(screen.getByRole("alert")).toHaveTextContent("This library selection is out of date.")
    const englishHeadings = headings()
    expect(englishHeadings.length).toBeGreaterThan(0)

    act(() => setLanguage("es"))

    // A surface that translates while it renders, with no hook of its own.
    expect(screen.getByText("Este proyecto aún no tiene sonidos")).toBeVisible()
    expect(screen.queryByText("This project has no sounds yet")).not.toBeInTheDocument()

    // An instrument editor made from a descriptor: group headings and parameter names.
    expect(headings()).toEqual(englishHeadings.map((heading) => translatedText(heading, "es")))
    expect(headings()).not.toEqual(englishHeadings)
    const named = fourOp.params.find((info) => catalogs.es[info.name as keyof typeof catalogs.es] !== info.name)
    expect(named).toBeDefined()
    expect(within(screen.getByTestId("editor")).queryAllByLabelText(named!.name, { exact: true })).toHaveLength(0)

    // A kept failure is shown in the new language, and stays unmarked as native text.
    expect(screen.getByRole("alert")).toHaveTextContent("Esta selección de la biblioteca está desactualizada.")
    expect(localizeBackendText(message("Unknown panel: {id}", { id: "mixer" }))).toBe("Panel desconocido: mixer")
    expect(localizeBackendText("an opaque engine failure")).toBe(message("Native diagnostic: {detail}", { detail: "an opaque engine failure" }))

    // A context menu: a registry action, an inline one and a disabled one with its reason.
    fireEvent.contextMenu(screen.getByTestId("area"))
    const menu = await screen.findByRole("menu")
    expect(within(menu).getByRole("menuitem", { name: /^Deshacer/ })).toBeVisible()
    expect(within(menu).getByRole("menuitem", { name: "Quitar efecto" })).toBeVisible()
    const disabled = within(menu).getByRole("menuitem", { name: /^Sustituir muestra desde un archivo de audio/ })
    expect(disabled).toHaveAttribute("aria-disabled", "true")
    expect(within(disabled).getByText("Solo samplers")).toBeVisible()
    await user.keyboard("{Escape}")

    // A dialog.
    act(() => useUiStore.getState().openDialog("settings"))
    expect(await screen.findByRole("dialog", { name: "Ajustes" })).toBeVisible()
    expect(screen.getByRole("combobox", { name: "Idioma de la interfaz" })).toBeVisible()
    act(() => useUiStore.getState().closeDialog())

    // The command palette: found by its Spanish name and still by its English one.
    act(() => useUiStore.getState().openDialog("palette"))
    const input = await screen.findByRole("combobox")
    expect(screen.getByRole("dialog", { name: "Paleta de comandos" })).toBeVisible()
    await user.type(input, "deshacer")
    expect(screen.getAllByRole("option")[0]).toHaveTextContent("Deshacer")
    await user.clear(input)
    await user.type(input, "undo")
    expect(screen.getAllByRole("option")[0]).toHaveTextContent("Deshacer")
    await user.clear(input)
    await user.type(input, "sustituir muestra desde un archivo")
    const row = screen.getAllByRole("option").find((option) => option.getAttribute("data-value") === "test.language.samplersOnly")
    expect(row).toBeDefined()
    expect(within(row!).getByText("Solo samplers")).toBeVisible()
    expect(searchActions(registry.list(), "Replace sample from an audio file", "es").map((action) => action.id)).toContain("test.language.samplersOnly")
  })

  it("shows a static failure from the backend in the interface language, whatever its first letter", () => {
    act(() => setLanguage("es"))
    expect(localizeBackendText(errorMessage("The clip no longer exists."))).toBe("El clip ya no existe.")
    expect(localizeBackendText(errorMessage("plugin capture was cancelled"))).toBe("Se canceló la captura del plugin")
    act(() => setLanguage("en"))
    expect(localizeBackendText(errorMessage("plugin capture was cancelled"))).toBe("Plugin capture was cancelled")
  })

  it("translates a built-in name inside a kept action title when it is shown", async () => {
    const kept = dynamicTitle("Add {value0}", { value0: "Compressor" }, { value0: "lower" })
    expect(kept.title).toBe("Add compressor")
    act(() => setLanguage("es"))
    expect(kept.title).toBe("Add compressor")
    render(<ContextActions items={[{ ...kept, run() {} }]}><div data-testid="area">Area</div></ContextActions>)
    fireEvent.contextMenu(screen.getByTestId("area"))
    const spanish = `Añadir ${translatedText("Compressor", "es").toLocaleLowerCase("es-ES")}`
    expect(await screen.findByRole("menuitem", { name: spanish })).toBeInTheDocument()
    expect(spanish).not.toBe("Añadir compressor")
  })
})
