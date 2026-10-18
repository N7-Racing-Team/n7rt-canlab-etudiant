// Board mission scenarios of the end-to-end harness (tools/stm32/n7rt_app.py): one implementation for the four
// missions. A scenario names its flow and its mission, mission-<id> (instructor), student-<id>, progression-<id>
// (release 0.3 pack, E0 to E5) or pack-<id> (a release 0.3 pack opened without any board), optionally prefixed by ide-
// (the integrated IDE builds and flashes the decoder first; never for pack-<id>).

export const E2E_MISSIONS = ["apps", "wsf", "sas", "bps"] as const;
export type E2eMission = (typeof E2E_MISSIONS)[number];

export interface MissionScenario {
  student: boolean;
  /** progression-<id>: the student flow of a release 0.3 pack, E0 to E5 (ui/src/e2e-progression.ts). */
  progression: boolean;
  /** False for pack-<id>: the student opens a release 0.3 pack on a machine without board. */
  board: boolean;
  id: E2eMission;
  /** Name used by the application and by the core's messages (« APPS »). */
  label: string;
  /** Two other missions: the student lock must refuse to open the first and to choose the second. */
  others: [E2eMission, E2eMission];
}

export function missionScenario(scenario: string): MissionScenario | null {
  const m = /^(mission|student|progression|pack)-([a-z]+)$/.exec(scenario);
  const id = m ? E2E_MISSIONS.find((x) => x === m[2]) : undefined;
  if (!m || !id) return null;
  const [first, second] = E2E_MISSIONS.filter((x) => x !== id);
  return {
    student: m[1] !== "mission", progression: m[1] === "progression" || m[1] === "pack", board: m[1] !== "pack",
    id, label: id.toUpperCase(), others: [first, second],
  };
}

/** Scenarios that run the integrated IDE workflow first: ide (then the functional check) or ide-<mission scenario>. */
export function ideScenario(scenario: string): boolean {
  return scenario === "ide" || (scenario.startsWith("ide-") && !!missionScenario(scenario.slice(4))?.board);
}

/** The core's message when the board announces a boot (engine.rs, spec 9.6). */
export const BOARD_REBOOTED = "La carte a redémarré";

export function isBoardReboot(e: { kind: string; text: string }): boolean {
  return e.kind === "session" && e.text.startsWith(BOARD_REBOOTED);
}
