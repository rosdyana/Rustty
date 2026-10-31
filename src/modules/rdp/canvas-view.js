// @ts-check
/**
 * Vista de una sesión RDP integrada: un `<canvas>` del tamaño del escritorio
 * remoto que pinta los rectángulos que cambian.
 *
 * Los mensajes llegan por un `Channel` de Tauri como bytes crudos. No se pintan
 * al llegar sino en el siguiente `requestAnimationFrame` (varios mensajes en un
 * mismo frame se pintan juntos), y tras pintar se confirma al backend el último
 * `seq`: sin esa confirmación el backend deja de enviar y acumula los cambios,
 * de modo que un webview lento no hace crecer la memoria.
 */

import { decodeFrame } from "./protocol.js";
import { attachInput } from "./input.js";

/**
 * @typedef {object} RdpCanvasView
 * @property {any} channel `Channel` para `rdp_embed_connect` (`onFrame`)
 * @property {HTMLElement} element contenedor del canvas
 * @property {() => void} focus
 * @property {(connected: boolean) => void} setActive
 * @property {() => { width: number, height: number, scale: number }} measure
 *   tamaño en píxeles físicos del hueco disponible y escala en %
 * @property {() => void} dispose
 */

/**
 * @param {object} deps
 * @param {string} deps.sessionId
 * @param {(cmd: string, args?: any, opts?: any) => Promise<any>} deps.invoke
 * @param {new () => any} deps.Channel
 * @param {(size: { width: number, height: number }) => void} [deps.onDesktopSize]
 * @returns {RdpCanvasView}
 */
export function createRdpCanvasView({ sessionId, invoke, Channel, onDesktopSize }) {
  const element = document.createElement("div");
  element.className = "rdp-canvas-host";
  const canvas = document.createElement("canvas");
  canvas.className = "rdp-canvas";
  canvas.width = 1;
  canvas.height = 1;
  const keyTarget = document.createElement("textarea");
  keyTarget.className = "rdp-key-target";
  keyTarget.setAttribute("aria-hidden", "true");
  keyTarget.setAttribute("autocomplete", "off");
  keyTarget.setAttribute("autocapitalize", "off");
  keyTarget.spellcheck = false;
  keyTarget.tabIndex = -1;
  element.append(canvas, keyTarget);

  const ctx = /** @type {CanvasRenderingContext2D} */ (canvas.getContext("2d", { alpha: false }));
  let active = false;
  let disposed = false;

  /** @type {Uint8Array[]} */
  let queue = [];
  let scheduled = false;
  let lastSeq = 0;

  const draw = () => {
    scheduled = false;
    if (disposed) return;
    const pending = queue;
    queue = [];
    let maxSeq = lastSeq;
    for (const bytes of pending) {
      // El `seq` se confirma aunque el mensaje no se entienda: si no, un
      // mensaje raro dejaría al backend esperando para siempre.
      if (bytes.byteLength >= 5) {
        maxSeq = new DataView(bytes.buffer, bytes.byteOffset, 5).getUint32(1, true);
      }
      const frame = decodeFrame(bytes);
      if (!frame) continue;
      switch (frame.type) {
        case "rects":
          for (const r of frame.rects) {
            ctx.putImageData(new ImageData(/** @type {any} */ (r.data), r.w, r.h), r.x, r.y);
          }
          break;
        case "desktop-size":
          // Cambiar el tamaño del canvas lo borra: el servidor repinta entero
          // tras una reactivación, así que no hace falta conservar nada.
          canvas.width = frame.width;
          canvas.height = frame.height;
          onDesktopSize?.({ width: frame.width, height: frame.height });
          break;
        case "pointer-bitmap":
          setCursorBitmap(canvas, frame);
          break;
        case "pointer-default":
          canvas.style.cursor = "default";
          break;
        case "pointer-hidden":
          canvas.style.cursor = "none";
          break;
      }
    }
    if (maxSeq !== lastSeq) {
      lastSeq = maxSeq;
      invoke("rdp_embed_ack", { sessionId, seq: maxSeq }).catch(() => {});
    }
  };

  const channel = new Channel();
  channel.onmessage = (/** @type {ArrayBuffer | Uint8Array | number[]} */ msg) => {
    if (disposed) return;
    const bytes = msg instanceof Uint8Array
      ? msg
      : msg instanceof ArrayBuffer
        ? new Uint8Array(msg)
        : Uint8Array.from(msg);
    queue.push(bytes);
    if (!scheduled) {
      scheduled = true;
      requestAnimationFrame(draw);
    }
  };

  const detachInput = attachInput({
    canvas,
    keyTarget,
    isActive: () => active && !disposed,
    send: (bytes) => {
      invoke("rdp_embed_input", bytes, { headers: { "x-session": sessionId } }).catch(() => {});
    },
  });

  return {
    channel,
    element,
    focus: () => keyTarget.focus({ preventScroll: true }),
    setActive: (connected) => {
      active = connected;
      element.classList.toggle("is-active", connected);
    },
    measure: () => {
      const dpr = window.devicePixelRatio || 1;
      // El hueco es el del panel: el contenedor del canvas está oculto hasta
      // que la sesión conecta.
      const r = (element.parentElement ?? element).getBoundingClientRect();
      return {
        width: Math.max(200, Math.floor(r.width * dpr)),
        height: Math.max(200, Math.floor(r.height * dpr)),
        scale: Math.round(dpr * 100),
      };
    },
    dispose: () => {
      disposed = true;
      active = false;
      queue = [];
      detachInput();
      channel.onmessage = () => {};
      element.remove();
    },
  };
}

/**
 * Pone el puntero remoto como cursor CSS del canvas.
 * @param {HTMLCanvasElement} canvas
 * @param {{ width: number, height: number, hotspotX: number, hotspotY: number, data: Uint8ClampedArray }} p
 */
function setCursorBitmap(canvas, p) {
  if (p.width === 0 || p.height === 0) {
    canvas.style.cursor = "none";
    return;
  }
  const c = document.createElement("canvas");
  c.width = p.width;
  c.height = p.height;
  const cctx = c.getContext("2d");
  if (!cctx) return;
  // Copia: el buffer del mensaje se recicla tras pintar.
  cctx.putImageData(new ImageData(new Uint8ClampedArray(p.data), p.width, p.height), 0, 0);
  canvas.style.cursor = `url(${c.toDataURL("image/png")}) ${p.hotspotX} ${p.hotspotY}, default`;
}
