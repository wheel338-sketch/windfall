import type { en } from "./en"
export type MessageKey = keyof typeof en
export type Message = string | { one: string; other: string }
export type Catalog = Record<MessageKey, Message>
export type Language = "en" | "es"
