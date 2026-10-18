import type { IdeView } from "./api";
import type { Status } from "../types";

/** The backend independently enforces these rules; the UI explains them before a click. */
export function flashRefusal(v: IdeView | null, dirty: boolean, board: Status | null, checking: boolean): string | null {
  if (!v?.available) return v?.reason ?? "Chargement du projet…";
  if (v.busy) return "Une opération est déjà en cours.";
  if (dirty || v.built_revision !== v.revision || !v.build?.ok) return "Compilez les modifications avant de flasher.";
  if (checking || board?.session) return "Arrêtez la session et attendez la fin de l'essai avant de flasher.";
  if (!board?.board?.runtime_verified || !board.board.compatible) return "Connectez une carte avec un runtime compatible et vérifié.";
  if (!board.port?.serial) return "La sonde ST-LINK de la carte doit être identifiée.";
  if (board.state !== "READY" && board.state !== "NO_STUDENT" && board.state !== "STUDENT_DISABLED") return "La carte doit être prête avant le flash.";
  return null;
}
