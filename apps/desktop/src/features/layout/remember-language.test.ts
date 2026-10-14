import { afterEach, expect, it, vi } from "vitest"
import { setLanguage } from "@/lib/i18n"
import { setBackend } from "@/lib/ipc"
import { createMockBackend } from "@/lib/ipc/mock"
import { rememberLanguage } from "./use-app-boot"

afterEach(() => {
  setLanguage("en")
  vi.restoreAllMocks()
})

it("tells the shell the interface language at start and on each change, for the exported plugin's panel", () => {
  const backend = createMockBackend({ storage: null })
  setBackend(backend)
  const remembered = vi.spyOn(backend, "rememberLanguage")
  const stop = rememberLanguage()
  expect(remembered).toHaveBeenLastCalledWith("en")
  setLanguage("es")
  expect(remembered).toHaveBeenLastCalledWith("es")
  // Setting the same language again tells it nothing new.
  setLanguage("es")
  expect(remembered).toHaveBeenCalledTimes(2)
  stop()
  setLanguage("en")
  expect(remembered).toHaveBeenCalledTimes(2)
})
