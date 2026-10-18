// Release integration test. It edits the real editor and clicks the real operation buttons.
import { e2e, lab } from "../bridge";
import { $ } from "../dom";
import type { E2eConfig } from "../types";
import type { Store } from "../store";
import { activeIde } from "./view";
import { ide } from "./api";
import { FILES } from "./model";

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export async function prepareIde(cfg: E2eConfig, store: Store, screenshots: boolean): Promise<unknown[]> {
  const checks: unknown[] = [];
  const until = async (label: string, test: () => boolean, timeout = 10000) => {
    const started = performance.now();
    while (!test()) { if (performance.now() - started > timeout) throw new Error(`IDE : délai dépassé (${label}).`); await sleep(30); }
  };
  const expect = (ok: boolean, message: string) => { if (!ok) throw new Error(message); };
  const click = (selector: string) => { const b = $<HTMLButtonElement>(selector); expect(!b.disabled, `${selector} désactivé.`); b.click(); };
  const shot = async (name: string) => {
    if (!screenshots || !cfg.coordinated) return;
    await sleep(200);
    await e2e.progress(`shot:${name}`);
    const started = performance.now();
    while (!(await e2e.released(`shot-${name}`))) { if (performance.now() - started > 15000) throw new Error(`Capture ${name} non libérée.`); await sleep(50); }
  };
  const cancelNativeClose = async (busy: boolean) => {
    await e2e.requestClose();
    await until("dialogue de fermeture native", () => !!document.querySelector<HTMLDialogElement>("#ide-close-dialog")?.open);
    expect(!!document.querySelector("#ide-close-save") === !busy, "La garde de fermeture ne distingue pas une source modifiée d'une opération active.");
    click("#ide-close-cancel");
    await until("fermeture annulée", () => !document.querySelector("#ide-close-dialog"));
    await sleep(60);
  };
  const operate = async (selector: string, testNativeClose = false) => {
    await until(`bouton ${selector}`, () => !$<HTMLButtonElement>(selector).disabled);
    click(selector);
    if (testNativeClose) {
      await until("opération active", () => activeIde!.busy);
      await cancelNativeClose(true);
      checks.push({ step: "native-close-busy-cancel", ok: true });
    }
    await until("fin opération", () => !activeIde!.busy, 120000);
    await sleep(120);
  };

  $('[data-show-view="code"]').click();
  await until("éditeur prêt", () => !!activeIde?.ready);
  const editor = activeIde!;
  // Save a user's preexisting work before temporarily exercising the template. Restore on completion/failure.
  const previous = FILES.map((name) => ({ name, source: editor.content(name) }));
  try {
    await e2e.progress("ide:project");
    await shot("01-project");
    if (screenshots) {
      click("#ide-restore"); click("#ide-restore-confirm");
      await until("template restauré", () => !editor.busy);
    }
    const base = editor.content("student_decoder.c");
    const reference = screenshots ? null : await e2e.studentSource();
    if (reference) {
      editor.edit("student_decoder.c", `${reference.c}\n/* CAN Lab IDE : source de validation fournie par l'opérateur. */\n`);
      if (reference.h !== null) editor.edit("student_decoder.h", reference.h);
    } else editor.edit("student_decoder.c", `${base.replace('n7rt_log("student_decoder ready");', 'n7rt_log("student_decoder ready");\n  n7rt_log("CAN Lab IDE build");')}\n/* Édition depuis CAN Lab : validation de la chaîne intégrée. */\n`);
    if (screenshots) {
      const c = editor.content("student_decoder.c");
      editor.edit("student_decoder.h", `${editor.content("student_decoder.h")}\n/* En-tête modifié dans CAN Lab. */\n`);
      editor.edit("student_decoder.c", c);
    }
    await sleep(120);
    expect(editor.modified, "L'éditeur n'a pas détecté la modification.");
    expect($<HTMLButtonElement>("#ide-flash").disabled, "Flash autorisé sur une source modifiée.");
    const unsaved = FILES.map((name) => editor.content(name));
    await e2e.progress("ide:close-unsaved");
    await cancelNativeClose(false);
    expect(editor.modified && FILES.every((name, index) => editor.content(name) === unsaved[index]), "La fermeture annulée a perdu le code non enregistré.");
    checks.push({ step: "native-close-unsaved-cancel", ok: true });
    await shot("02-edit");
    await e2e.progress("ide:build");
    await operate("#ide-build", true);
    expect(!!editor.state?.build?.ok, editor.failure || "Compilation IDE échouée.");
    checks.push({ step: "build", ok: true, artifact: editor.state?.build?.artifact });
    await shot("03-build");

    if (screenshots) {
      const good = editor.content("student_decoder.c");
      editor.edit("student_decoder.c", `${good}\n#error N7RT_TEST_DIAGNOSTIC\n`);
      await e2e.progress("ide:compile-error");
      await operate("#ide-build");
      expect(editor.state?.build?.ok === false, "La compilation invalide a été acceptée.");
      expect(!!editor.state?.diagnostics.some((d) => d.file === "student_decoder.c" && d.line && d.message.includes("N7RT_TEST_DIAGNOSTIC")), "Erreur GCC sans fichier/ligne utile.");
      expect($<HTMLButtonElement>("#ide-flash").disabled, "Flash d'un ancien binaire autorisé après échec.");
      $('[data-view="code"] [data-tab="ide-problems"]').click();
      await shot("04-diagnostics");
      editor.edit("student_decoder.c", good);
      $('[data-view="code"] [data-tab="ide-console"]').click();
      await operate("#ide-build");
      expect(!!editor.state?.build?.ok, "Correction non compilée.");
      checks.push({ step: "diagnostics-and-stale-refusal", ok: true });
    }
    await lab.refresh();
    await until("carte détectée", () => !!store.status?.ports.some((p) => p.serial === cfg.serial));
    await lab.connect(null, cfg.serial);
    await until("runtime vérifié", () => !!store.status?.board?.runtime_verified && ["READY", "NO_STUDENT", "STUDENT_DISABLED"].includes(store.status.state), 15000);
    const uid = store.status!.board!.uid;
    await sleep(150);
    await e2e.progress("ide:flash");
    await shot("05-flash");
    await operate("#ide-flash");
    expect(!!editor.state?.flash?.ok, editor.failure || "Flash IDE échoué.");
    await until("CRC carte mis à jour", () => {
      const loaded = store.status?.board?.student_crc32;
      return !!loaded && parseInt(loaded.replace(/^0x/i, ""), 16) === editor.state?.build?.artifact?.student_crc32 && store.status?.state === "READY";
    }, 15000);
    expect(store.status?.board?.uid === uid && store.status.board.runtime_verified, "Identité/intégrité différente après flash.");
    await sleep(200);
    expect($("#ide-match").textContent === "La carte exécute la compilation actuelle.", "L'UI ne confirme pas le CRC actuel.");
    checks.push({ step: "flash-reconnect-ui", ok: true, uid, crc: store.status!.board!.student_crc32 });
    if (screenshots) {
      editor.edit("student_decoder.c", `${editor.content("student_decoder.c")}\n/* Validation du bouton Compiler + flasher. */\n`);
      await operate("#ide-build-flash");
      expect(!!editor.state?.build?.ok && !!editor.state?.flash?.ok, "Compiler + flasher a échoué.");
      await until("reconnexion compilation combinée", () => store.status?.state === "READY" && store.status.board?.runtime_verified === true, 15000);
      checks.push({ step: "both-authorised-files-and-combined-button", ok: true });
    }
    await shot("06-reconnected");
    $('[data-show-view="cockpit"]').click();
    return checks;
  } finally {
    // No automatic hardware restore: just preserve source text. Later flash always needs a new build.
    if (!editor.busy) {
      for (const file of previous) editor.edit(file.name, file.source);
      await editor.run("save");
    }
    await ide.state();
  }
}
