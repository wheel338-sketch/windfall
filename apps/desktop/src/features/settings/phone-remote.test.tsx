import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { setBackend } from "@/lib/ipc"
import { startTestApp } from "@/test/harness"
import { PhoneRemoteSettings } from "./phone-remote"

describe("phone remote settings", () => {
  let app: Awaited<ReturnType<typeof startTestApp>>
  beforeEach(async () => { app = await startTestApp() })
  afterEach(() => { app.stop() })
  it("starts only on explicit enable, shows pairing, and revokes on disable", async () => {
    const off = { enabled: false, address: null, pairingCode: null }
    const phoneRemoteSet = vi.fn(async (address: string | null) => address ? { enabled: true, address: `http://${address}`, pairingCode: "123456" } : off)
    setBackend({ ...app.backend, kind: "tauri", phoneRemoteState: async () => off, phoneRemoteSet })
    render(<PhoneRemoteSettings />)
    await waitFor(() => expect(screen.getByRole("button", { name: "Enable phone remote" })).toBeEnabled())
    expect(phoneRemoteSet).not.toHaveBeenCalled()
    const user = userEvent.setup()
    await user.clear(screen.getByLabelText("Local IPv4 address and port"))
    await user.type(screen.getByLabelText("Local IPv4 address and port"), "192.168.1.4:8787")
    await user.click(screen.getByRole("button", { name: "Enable phone remote" }))
    expect(await screen.findByText("123456")).toBeInTheDocument()
    expect(phoneRemoteSet).toHaveBeenCalledWith("192.168.1.4:8787")
    await user.click(screen.getByRole("button", { name: "Disable phone remote" }))
    await waitFor(() => expect(phoneRemoteSet).toHaveBeenLastCalledWith(null))
  })
  it("shows native capability and bind errors without simulated success", async () => {
    render(<PhoneRemoteSettings />)
    expect(await screen.findByText("This feature requires the Windfall desktop app.")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Enable phone remote" })).toBeDisabled()
  })
  it("reports a refused bind and keeps the listener disabled", async () => {
    const phoneRemoteSet=vi.fn(async () => {throw new Error("Remote binding requires a specific private IPv4 address.")})
    setBackend({...app.backend,kind:"tauri",phoneRemoteState:async()=>({enabled:false,address:null,pairingCode:null}),phoneRemoteSet})
    render(<PhoneRemoteSettings />)
    await waitFor(()=>expect(screen.getByRole("button",{name:"Enable phone remote"})).toBeEnabled())
    await userEvent.setup().click(screen.getByRole("button",{name:"Enable phone remote"}))
    expect(await screen.findByText("Remote binding requires a specific private IPv4 address.")).toBeInTheDocument()
    expect(screen.getByRole("button",{name:"Enable phone remote"})).toBeEnabled()
  })
})
