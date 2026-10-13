import type { LibraryFileToken } from "@/bindings"
import { backend } from "@/lib/ipc"
import { translatedText as t } from "@/lib/i18n"

import { useLibraryStore } from "./library-store"
import { useBrowserStore, type Selection } from "./store"
import { isUnder } from "./tree-model"

const STALE =
  "This library selection is out of date. Refresh the library and select the file again."

export type FileRequest = Readonly<{
  path: string
  epoch: number
  rootPath?: string
  browser?: LibraryFileToken
  selection?: Selection
}>

/** Capture once, before any timer or await; never resolve from a later selection. */
export function captureFile(
  path: string,
  browser?: LibraryFileToken
): FileRequest {
  const selected = useBrowserStore.getState().selected
  const selection = selected?.path === path ? selected : undefined
  return Object.freeze({
    path,
    epoch:
      selection?.librarySelection?.epoch ?? useLibraryStore.getState().epoch,
    rootPath: selection?.librarySelection?.rootPath ?? browser?.rootPath,
    browser: browser ?? selection?.library,
    selection,
  })
}

export function fileRequestCurrent(request: FileRequest): boolean {
  return request.epoch === useLibraryStore.getState().epoch
}

/** A successful preview/facts lookup pins the selection independently of search. */
export async function resolveFile(
  request: FileRequest
): Promise<LibraryFileToken> {
  if (!fileRequestCurrent(request)) throw new Error(t(STALE))
  const token = request.browser ?? (await backend.libraryFile(request.path))
  return acceptFile(request, token)
}

/** Preserve immediate audition when the selection already has a token. */
export function acceptFile(
  request: FileRequest,
  token: LibraryFileToken
): LibraryFileToken {
  // library_file picks the first configured containing root. A nested tree
  // row can belong to another root of the same file; preserve that row's
  // captured epoch/root while using the backend's canonical checked token.
  const roots = useBrowserStore.getState().roots
  const contains = (rootPath: string) =>
    roots.some(
      (root) => root.path === rootPath && isUnder(request.path, rootPath)
    )
  if (
    !fileRequestCurrent(request) ||
    token.path !== request.path ||
    (request.rootPath !== undefined &&
      token.rootPath !== request.rootPath &&
      !(contains(request.rootPath) && contains(token.rootPath)))
  )
    throw new Error(t(STALE))
  if (
    request.selection &&
    useBrowserStore.getState().selected === request.selection
  ) {
    useBrowserStore.setState({
      selected: { ...request.selection, library: token },
    })
  }
  return token
}
