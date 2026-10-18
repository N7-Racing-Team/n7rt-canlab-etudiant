import { invoke } from "@tauri-apps/api/core";
import type { StudentFile } from "./model";

export interface Diagnostic { severity: string; file: string | null; line: number | null; column: number | null; message: string }
export interface Artifact { sha256: string; student_crc32: number; flash_bytes: number; ram_bytes: number }
export interface ToolResult { ok: boolean; operation: string; diagnostics: Diagnostic[]; checks: unknown[]; artifact: Artifact | null; message?: string }
export interface IdeView {
  available: boolean; reason: string | null; busy: boolean; phase: "idle" | "building" | "flashing" | "reconnecting";
  revision: number; built_revision: number | null; files: StudentFile[]; diagnostics: Diagnostic[];
  build: ToolResult | null; flash: ToolResult | null;
}
export const ide = {
  state: () => invoke<IdeView>("ide_state"),
  read: (name: StudentFile) => invoke<{ name: StudentFile; content: string; revision: number }>("ide_read", { name }),
  save: (name: StudentFile, content: string) => invoke<IdeView>("ide_save", { name, content }),
  restore: () => invoke<IdeView>("ide_restore"),
  build: () => invoke<IdeView>("ide_build"),
  flash: () => invoke<IdeView>("ide_flash"),
};
