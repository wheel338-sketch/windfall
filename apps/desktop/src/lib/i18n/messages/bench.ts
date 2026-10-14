import type { Message } from "../types"

export const benchEn = {
  "Continuous horizontal scroll through the whole song and back. 8 bars across the view, 16 px rows.": "Continuous horizontal scroll through the whole song and back. 8 bars across the view, 16 px rows.",
  "Dragging 1,000 selected notes in time and pitch. The move is a drag offset on the view; no note data changes until the drop.": "Dragging 1,000 selected notes in time and pitch. The move is a drag offset on the view; no note data changes until the drop.",
  "Every note on screen at once: the whole song across the view and all 88 keys, with a small zoom and scroll wobble so each frame is a full redraw.": "Every note on screen at once: the whole song across the view and all 88 keys, with a small zoom and scroll wobble so each frame is a full redraw.",
  "Only the playhead moves, at 140 BPM. The grid and the notes are not redrawn.": "Only the playhead moves, at 140 BPM. The grid and the notes are not redrawn.",
  "Worst case for an edit: every frame 1,000 notes change in the model, the whole batch is rebuilt from the Note array (colors, sort, index) and uploaded again.": "Worst case for an edit: every frame 1,000 notes change in the model, the whole batch is rebuilt from the Note array (colors, sort, index) and uploaded again.",
  "Zoom sweep around the middle of the song, from all 200 bars across the view to 2 bars and back.": "Zoom sweep around the middle of the song, from all 200 bars across the view to 2 bars and back.",
  "bench.html has no #root element": "bench.html has no #root element",
} as const

export const benchEs = {
  "Continuous horizontal scroll through the whole song and back. 8 bars across the view, 16 px rows.": "Desplazamiento horizontal continuo por toda la canción, ida y vuelta. 8 compases a lo ancho de la vista, filas de 16 px.",
  "Dragging 1,000 selected notes in time and pitch. The move is a drag offset on the view; no note data changes until the drop.": "Arrastre de 1.000 notas seleccionadas en tiempo y afinación. El movimiento es un desplazamiento de arrastre en la vista; los datos de las notas no cambian hasta soltar.",
  "Every note on screen at once: the whole song across the view and all 88 keys, with a small zoom and scroll wobble so each frame is a full redraw.": "Todas las notas en pantalla a la vez: la canción entera a lo ancho de la vista y las 88 teclas, con una ligera oscilación de ampliación y desplazamiento para que cada fotograma sea un redibujado completo.",
  "Only the playhead moves, at 140 BPM. The grid and the notes are not redrawn.": "Solo se mueve el cursor de reproducción, a 140 BPM. La cuadrícula y las notas no se redibujan.",
  "Worst case for an edit: every frame 1,000 notes change in the model, the whole batch is rebuilt from the Note array (colors, sort, index) and uploaded again.": "Peor caso de una edición: en cada fotograma cambian 1.000 notas en el modelo, y todo el lote se reconstruye a partir de la matriz de Note (colores, orden, índice) y se vuelve a subir.",
  "Zoom sweep around the middle of the song, from all 200 bars across the view to 2 bars and back.": "Barrido de ampliación alrededor del centro de la canción: de los 200 compases a lo ancho de la vista hasta 2 compases, ida y vuelta.",
  "bench.html has no #root element": "bench.html no tiene ningún elemento #root",
} satisfies Record<keyof typeof benchEn, Message>
