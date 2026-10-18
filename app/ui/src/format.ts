// Cell formats. crates/n7rt-lab/src/e2e.rs parses exactly these texts back to verify that what
// is displayed equals what the board answered: change both together.

import type { EntryView, FrameRow, LinkState } from "./types";

const CAN_EXT = 0x01, CAN_RTR = 0x02, CAN_FD = 0x04, CAN_BRS = 0x08, CAN_ESI = 0x10;

const scratch = new DataView(new ArrayBuffer(4));

export function f32FromBits(bits: number): number {
  scratch.setUint32(0, bits >>> 0, true);
  return scratch.getFloat32(0, true);
}

/** Shortest decimal text that reads back as the same binary32 value. */
export function f32Text(bits: number): string {
  const v = f32FromBits(bits);
  if (Number.isNaN(v)) return "NaN";
  if (v === Infinity) return "inf";
  if (v === -Infinity) return "-inf";
  if (v === 0) return Object.is(v, -0) ? "-0" : "0";
  for (let p = 1; p <= 9; p++) {
    const s = Number(v.toPrecision(p));
    if (Math.fround(s) === v) return String(s);
  }
  return String(v);
}

export function hex(n: number, width: number): string {
  return n.toString(16).toUpperCase().padStart(width, "0");
}

export function sessionText(id: number): string {
  return hex(id >>> 0, 8);
}

export function idText(id: number, flags: number): string {
  return "0x" + hex(id, flags & CAN_EXT ? 8 : 3);
}

export function kindText(flags: number): string {
  const t = [flags & CAN_EXT ? "EXT" : "STD"];
  if (flags & CAN_FD) t.push("FD");
  if (flags & CAN_BRS) t.push("BRS");
  if (flags & CAN_ESI) t.push("ESI");
  if (flags & CAN_RTR) t.push("RTR");
  return t.join(" ");
}

/** "11 22 33" from "112233"; "—" when there is no data byte. */
export function dataText(h: string): string {
  if (!h) return "—";
  const out: string[] = [];
  for (let i = 0; i < h.length; i += 2) out.push(h.slice(i, i + 2).toUpperCase());
  return out.join(" ");
}

export function resultText(r: FrameRow): string {
  switch (r.outcome) {
    case "result": {
      let s = r.status === 0 ? "OK" : "non livrée";
      if (r.result_flags & 0x01) s += " +débordement";
      if (r.result_flags & 0x02) s += " +budget";
      return s;
    }
    case "nack":
      return `NACK ${r.nack ?? "?"}`;
    case "lost":
      return "perdue";
    case "aborted":
      return "abandonnée";
  }
}

export function entryText(e: EntryView): string {
  return `s${e.slot}=${e.kind === 1 ? "rejet" : f32Text(e.bits)}`;
}

export function publicationsText(entries: EntryView[]): string {
  return entries.length ? entries.map(entryText).join(" · ") : "—";
}

export function msText(us: number, digits = 3): string {
  return (us / 1000).toFixed(digits);
}

export function durationText(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  return `${m}:${String(s % 60).padStart(2, "0")}`;
}

export const STATE_TEXT: Record<LinkState, string> = {
  DISCONNECTED: "Déconnecté",
  ABSENT: "Carte absente",
  PORT_BUSY: "Port occupé",
  HANDSHAKE: "Poignée de main",
  READY: "Prête",
  NO_STUDENT: "Pas de code étudiant",
  STUDENT_DISABLED: "Code étudiant désactivé",
  INCOMPATIBLE: "Carte incompatible",
  SESSION: "Session active",
  LOST: "Liaison perdue",
};

/** Badge / status-bar variant per state (ui/DESIGN.md, États). Running = N7RT violet. */
export const STATE_CLASS: Record<LinkState, "" | "ok" | "accent" | "info" | "warn" | "err"> = {
  DISCONNECTED: "",
  ABSENT: "warn",
  PORT_BUSY: "err",
  HANDSHAKE: "info",
  READY: "ok",
  NO_STUDENT: "warn",
  STUDENT_DISABLED: "warn",
  INCOMPATIBLE: "err",
  SESSION: "accent",
  LOST: "err",
};

/** Data length implied by a DLC (spec section 8.2), for the DLC selector labels. */
export function dataLength(dlc: number, format: string): number {
  if (format === "remote") return 0;
  const fd = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64];
  return format === "fd" || format === "fd_brs" ? fd[dlc] : Math.min(dlc, 8);
}
