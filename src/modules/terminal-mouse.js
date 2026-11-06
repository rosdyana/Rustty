// @ts-check
/**
 * Decisión del clic derecho sobre el terminal. Núcleo **puro**: no toca DOM ni
 * xterm. El llamador lee el estado (selección, enlace bajo el puntero, pref de
 * pegado) y ejecuta la acción que se devuelve.
 */

/**
 * Acción del clic derecho en el terminal:
 * - `"ignore"`: el puntero está sobre un enlace; no se activa ni se pega.
 * - `"copy"`: con pegado con clic derecho y hay selección → copiarla.
 * - `"paste"`: con pegado con clic derecho y sin selección → pegar.
 * - `"menu"`: sin pegado con clic derecho → menú contextual del terminal.
 * @typedef {"ignore" | "copy" | "paste" | "menu"} TerminalRightClickAction
 */

/**
 * @param {{ rightClickPaste: boolean, hasSelection: boolean, linkHovered: boolean }} state
 * @returns {TerminalRightClickAction}
 */
export function terminalRightClickAction({ rightClickPaste, hasSelection, linkHovered }) {
  if (linkHovered) return "ignore";
  if (rightClickPaste) return hasSelection ? "copy" : "paste";
  return "menu";
}
