import type { Message } from "../types"

export const stageEn = {
  "Clear master clip indicator": "Clear master clip indicator",
  "Large clock": "Large clock",
  "Large master meter": "Large master meter",
  "Left peak level": "Left peak level",
  "Master peak levels. Click the meter to clear its clip light.": "Master peak levels. Click the meter to clear its clip light.",
  "Peak level · −60 to +6 dBFS": "Peak level · −60 to +6 dBFS",
  "Right peak level": "Right peak level",
  "Stereo master meter": "Stereo master meter",
  "Transport position in bars, beats and steps, and minutes and seconds.": "Transport position in bars, beats and steps, and minutes and seconds.",
} as const

export const stageEs = {
  "Clear master clip indicator": "Borrar el indicador de saturación maestro",
  "Large clock": "Reloj grande",
  "Large master meter": "Medidor maestro grande",
  "Left peak level": "Nivel de pico izquierdo",
  "Master peak levels. Click the meter to clear its clip light.": "Niveles de pico maestros. Haz clic en el medidor para borrar su indicador de saturación.",
  "Peak level · −60 to +6 dBFS": "Nivel de pico · de −60 a +6 dBFS",
  "Right peak level": "Nivel de pico derecho",
  "Stereo master meter": "Medidor maestro estéreo",
  "Transport position in bars, beats and steps, and minutes and seconds.": "Posición del transporte en compases, pulsos y pasos, y en minutos y segundos.",
} satisfies Record<keyof typeof stageEn, Message>
