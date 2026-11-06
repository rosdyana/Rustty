// @ts-check
import { describe, expect, it } from "vitest";
import { terminalRightClickAction } from "./terminal-mouse.js";

describe("terminalRightClickAction", () => {
  it("ignora el clic derecho sobre un enlace, con o sin pegado y selección", () => {
    expect(terminalRightClickAction({ rightClickPaste: false, hasSelection: false, linkHovered: true })).toBe("ignore");
    expect(terminalRightClickAction({ rightClickPaste: true, hasSelection: false, linkHovered: true })).toBe("ignore");
    expect(terminalRightClickAction({ rightClickPaste: true, hasSelection: true, linkHovered: true })).toBe("ignore");
  });

  it("con pegado con clic derecho copia si hay selección", () => {
    expect(terminalRightClickAction({ rightClickPaste: true, hasSelection: true, linkHovered: false })).toBe("copy");
  });

  it("con pegado con clic derecho pega si no hay selección", () => {
    expect(terminalRightClickAction({ rightClickPaste: true, hasSelection: false, linkHovered: false })).toBe("paste");
  });

  it("sin pegado con clic derecho abre el menú contextual, con o sin selección", () => {
    expect(terminalRightClickAction({ rightClickPaste: false, hasSelection: false, linkHovered: false })).toBe("menu");
    expect(terminalRightClickAction({ rightClickPaste: false, hasSelection: true, linkHovered: false })).toBe("menu");
  });
});
