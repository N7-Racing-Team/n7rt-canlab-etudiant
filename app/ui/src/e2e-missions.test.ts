// Mission scenarios of the end-to-end harness (e2e-missions.ts): one parser for the four missions, the ide- prefix and
// the core's board reboot message.

import { describe, expect, it } from "vitest";
import { BOARD_REBOOTED, E2E_MISSIONS, ideScenario, isBoardReboot, missionScenario } from "./e2e-missions";

describe("missionScenario", () => {
  it("names the flow, the mission and two other missions for each of the four", () => {
    for (const id of E2E_MISSIONS) {
      const instructor = missionScenario(`mission-${id}`);
      const student = missionScenario(`student-${id}`);
      expect(instructor).toMatchObject({ student: false, id, label: id.toUpperCase() });
      expect(student).toMatchObject({ student: true, id, label: id.toUpperCase() });
      expect(new Set([id, ...instructor!.others]).size).toBe(3);
      expect(instructor!.others.every((m) => E2E_MISSIONS.includes(m))).toBe(true);
    }
  });

  it("plays a release 0.3 pack from E0 to E5 on a board, or opens it without any board", () => {
    for (const id of E2E_MISSIONS) {
      expect(missionScenario(`progression-${id}`)).toMatchObject({ student: true, progression: true, board: true, id });
      expect(missionScenario(`pack-${id}`)).toMatchObject({ student: true, progression: true, board: false, id });
      expect(missionScenario(`student-${id}`)).toMatchObject({ progression: false, board: true });
    }
  });

  it("keeps the historical APPS checks: WSF refused at opening, SAS refused at choice", () => {
    expect(missionScenario("student-apps")!.others).toEqual(["wsf", "sas"]);
  });

  it("rejects anything that is not a known mission scenario", () => {
    for (const s of ["mission-", "mission-xyz", "student-APPS", "mission-apps-2", "ide-mission-apps", "functional", "phys", "", "missionapps"]) {
      expect(missionScenario(s)).toBeNull();
    }
  });
});

describe("ideScenario", () => {
  it("accepts ide and ide- followed by a mission scenario only", () => {
    expect(ideScenario("ide")).toBe(true);
    for (const id of E2E_MISSIONS) {
      expect(ideScenario(`ide-mission-${id}`)).toBe(true);
      expect(ideScenario(`ide-student-${id}`)).toBe(true);
    }
    for (const id of E2E_MISSIONS) expect(ideScenario(`ide-progression-${id}`)).toBe(true);
    for (const s of ["ide-", "ide-functional", "ide-mission-xyz", "mission-apps", "ide-phys", "ide-pack-apps"]) expect(ideScenario(s)).toBe(false);
  });
});

describe("isBoardReboot", () => {
  it("matches the core's session error for a board boot, not other messages", () => {
    expect(isBoardReboot({ kind: "session", text: `${BOARD_REBOOTED} (reset logiciel, broche NRST).` })).toBe(true);
    expect(isBoardReboot({ kind: "link", text: `${BOARD_REBOOTED} (reset logiciel).` })).toBe(false);
    expect(isBoardReboot({ kind: "session", text: "La carte n'a pas confirmé l'ouverture de session (4 essais)." })).toBe(false);
  });
});
