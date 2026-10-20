// Real CodeMirror editor; the operation latch complements independent backend checks.
import { basicSetup } from "codemirror";
import { cpp } from "@codemirror/lang-cpp";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { indentWithTab } from "@codemirror/commands";
import { HighlightStyle, indentUnit, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { openSearchPanel } from "@codemirror/search";
import { lintGutter, setDiagnostics } from "@codemirror/lint";
import { autocompletion, completeFromList } from "@codemirror/autocomplete";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { $, clear, el, setText } from "../dom";
import type { MissionView, Status } from "../types";
import { closeDialog, ficheUnsaved, settleFiche, type CloseChoice } from "../views/unsaved";
import { ide, type Diagnostic, type IdeView, type ToolResult } from "./api";
import { flashRefusal } from "./controls";
import { FILES, compilerPositionOffset, cursorPosition, failureText, outline, positionOffset, type StudentFile } from "./model";

const MAX_SOURCE_BYTES = 256 * 1024;
const PHASE = { idle: "Prêt", building: "Compilation en cours…", flashing: "Programmation de la zone étudiante…", reconnecting: "Reconnexion et vérification de la carte…" };
const crc = (n: number) => `0x${(n >>> 0).toString(16).toUpperCase().padStart(8, "0")}`;
const completions = completeFromList([
  { label: "n7rt_publish", type: "function", detail: "(uint8_t slot, float value)" },
  { label: "n7rt_reject", type: "function", detail: "(uint8_t slot)" },
  { label: "n7rt_log", type: "function", detail: "(const char *message)" },
  { label: "n7rt_can_frame_t", type: "type" },
  { label: "n7rt_now_us", type: "function", detail: "uint64_t (void)" },
  ...["student_init", "student_on_frame", "student_on_tick"].map((label) => ({ label, type: "function" })),
]);

export let activeIde: StudentIde | null = null;

export class StudentIde {
  private current: StudentFile = "student_decoder.c";
  private editor: EditorView | null = null;
  private states = new Map<StudentFile, EditorState>();
  private saved = new Map<StudentFile, string>();
  private view: IdeView | null = null;
  private board: Status | null = null;
  private checking = false;
  private visible = false;
  private operation = false;
  private renderDirty = true;
  private initialised = false;
  private polling = false;
  private nextPoll = 0;
  private lock = new Compartment();
  private consoleLines: string[] = [];
  /** The operation of the last result shown ("build" | "flash"): names the problems located in no file. */
  private lastOperation = "build";
  private error = "";
  private lastResultKey = "";
  private stateEpoch = 0;
  private closeGuard: Promise<void> | null = null;
  private closePending = false;
  private closing = false;

  constructor(private navigate: (view: string) => void) {}

  mount(): void {
    activeIde = this;
    for (const b of Array.from(document.querySelectorAll<HTMLButtonElement>('[data-view="code"] [data-file]'))) {
      b.addEventListener("click", () => this.openFile(b.dataset.file as StudentFile));
    }
    $("#ide-save").addEventListener("click", () => void this.run("save"));
    $("#ide-build").addEventListener("click", () => void this.run("build"));
    $("#ide-flash").addEventListener("click", () => void this.run("flash"));
    $("#ide-build-flash").addEventListener("click", () => void this.run("both"));
    $("#ide-search").addEventListener("click", () => { if (this.editor) openSearchPanel(this.editor); });
    $("#ide-cockpit").addEventListener("click", () => this.navigate("cockpit"));
    $("#ide-phys").addEventListener("click", () => this.navigate("phys"));
    const dialog = $<HTMLDialogElement>("#ide-restore-dialog");
    $("#ide-restore").addEventListener("click", () => dialog.showModal());
    $("#ide-restore-cancel").addEventListener("click", () => dialog.close());
    $("#ide-restore-confirm").addEventListener("click", () => { dialog.close(); void this.run("restore"); });
    window.addEventListener("beforeunload", (event) => {
      if (!this.closing && (this.modified || this.busy || ficheUnsaved())) { event.preventDefault(); event.returnValue = ""; }
    });
  }

  /** One native close listener for the whole window, including when the editor is hidden. */
  protectNativeClose(): Promise<void> {
    this.closeGuard ??= getCurrentWindow().onCloseRequested(async (event) => {
      // Always stop the native close before the first asynchronous operation.
      event.preventDefault();
      await this.requestNativeClose(() => getCurrentWindow().destroy());
    }).then(() => undefined);
    return this.closeGuard;
  }

  private closeChoice(message: string, modified: boolean, title = modified ? "Code non enregistré" : "Fermeture suspendue"): Promise<CloseChoice> {
    return closeDialog("ide-close", title, message, modified, "Revenir à l'application");
  }

  private async requestNativeClose(destroy: () => Promise<void>): Promise<void> {
    if (this.closePending) return;
    this.closePending = true;
    try {
      const backend = await ide.state();
      if (this.operation || backend.busy) {
        await this.closeChoice("Une opération est en cours. Attendez sa fin avant de fermer l'application.", false);
        return;
      }
      if (this.modified) {
        const choice = await this.closeChoice("Vos modifications ne sont pas encore enregistrées. Que souhaitez-vous faire ?", true);
        if (choice === "cancel") return;
        if (choice === "save") {
          await this.run("save");
          if (this.modified || this.error) {
            await this.closeChoice("Le code n'a pas pu être enregistré. Revenez à l'éditeur pour corriger le problème.", false);
            return;
          }
        }
      }
      // the fiche's unsaved changes (views/unsaved.ts): saved, or discarded on purpose
      if (!(await settleFiche((message, offer) => this.closeChoice(message, offer, offer ? "Fiche non enregistrée" : "Fermeture suspendue")))) return;
      // A save, another UI control or an external call could start work during the dialog.
      const latest = await ide.state();
      if (this.operation || latest.busy) {
        await this.closeChoice("Une opération a démarré. Attendez sa fin avant de fermer l'application.", false);
        return;
      }
      this.closing = true;
      try { await destroy(); }
      catch { this.closing = false; throw new Error("Fermeture impossible."); }
    } catch {
      this.error = "La fermeture a été suspendue : vérifiez que votre code est enregistré, puis réessayez.";
      this.appendConsole(this.error);
      await this.closeChoice(this.error, false);
    } finally {
      this.closePending = false;
    }
  }

  show(): void { this.visible = true; this.renderDirty = true; if (!this.initialised) void this.load(); else this.editor?.requestMeasure(); }
  hide(): void { this.visible = false; }

  setBoard(board: Status | null, mission: MissionView | null): void {
    const before = JSON.stringify([this.board?.state, this.board?.board, !!this.board?.session, this.checking]);
    this.board = board;
    this.checking = mission?.check.state === "running";
    if (JSON.stringify([board?.state, board?.board, !!board?.session, this.checking]) !== before) this.renderDirty = true;
  }

  frame(now: number): void {
    if (this.visible && this.initialised && !this.polling && now >= this.nextPoll) {
      this.nextPoll = now + 500;
      this.polling = true;
      const epoch = this.stateEpoch;
      void ide.state().then((v) => {
        if (epoch !== this.stateEpoch) return;
        if (JSON.stringify(v) !== JSON.stringify(this.view)) { this.view = v; this.renderDirty = true; }
      }).catch(() => {}).finally(() => { this.polling = false; });
    }
    if (this.visible && this.renderDirty) { this.renderDirty = false; this.render(); }
  }

  get modified(): boolean { return this.initialised && FILES.some((name) => this.content(name) !== this.saved.get(name)); }
  get state(): IdeView | null { return this.view; }
  get busy(): boolean { return this.operation || !!this.view?.busy; }
  get ready(): boolean { return this.initialised && !!this.editor; }
  get failure(): string { return this.error; }
  content(name: StudentFile): string { return (name === this.current ? this.editor?.state : this.states.get(name))?.doc.toString() ?? ""; }

  /** Also used by release e2e tests: an ordinary editor transaction with undo history. */
  edit(name: StudentFile, source: string): void {
    if (this.busy || !this.editor) throw new Error("Éditeur occupé.");
    this.openFile(name);
    this.editor.dispatch({ changes: { from: 0, to: this.editor.state.doc.length, insert: source } });
  }

  private extensions() {
    const nonce = document.querySelector<HTMLMetaElement>('meta[name="n7rt-editor-style-nonce"]')?.content ?? "";
    return [
      basicSetup, cpp(), lintGutter(), indentUnit.of("  "), EditorView.cspNonce.of(nonce),
      syntaxHighlighting(HighlightStyle.define([
        { tag: tags.keyword, class: "ide-syntax-keyword" },
        { tag: [tags.string, tags.character], class: "ide-syntax-string" },
        { tag: tags.comment, class: "ide-syntax-comment" },
        { tag: tags.number, class: "ide-syntax-number" },
        { tag: [tags.typeName, tags.macroName], class: "ide-syntax-type" },
      ])),
      EditorView.theme({}, { dark: true }),
      this.lock.of(EditorState.readOnly.of(false)),
      EditorView.contentAttributes.of({ "aria-label": "Code C étudiant", spellcheck: "false" }),
      autocompletion({ override: [completions] }),
      keymap.of([{ key: "Mod-s", run: () => { void this.run("save"); return true; } }, { key: "Mod-h", run: openSearchPanel }, indentWithTab]),
      EditorView.updateListener.of((u) => {
        if (u.docChanged || u.selectionSet) { this.states.set(this.current, u.state); this.renderDirty = true; }
      }),
    ];
  }

  private async load(): Promise<void> {
    if (this.operation) return;
    this.operation = true;
    this.stateEpoch++;
    this.renderDirty = true;
    try {
      this.view = await ide.state();
      if (!this.view.available) throw new Error(this.view.reason ?? "Projet étudiant indisponible.");
      for (const name of FILES) {
        const file = await ide.read(name);
        this.saved.set(name, file.content);
        this.states.set(name, EditorState.create({ doc: file.content, extensions: this.extensions() }));
      }
      this.editor = new EditorView({ state: this.states.get(this.current)!, parent: $("#ide-editor") });
      this.initialised = true;
      this.appendConsole("Projet étudiant ouvert. Modifiez le code, puis compilez.");
      this.error = "";
    } catch (err) { this.error = String(err); }
    finally { this.operation = false; this.stateEpoch++; this.renderDirty = true; }
  }

  private openFile(name: StudentFile): void {
    if (!FILES.includes(name) || !this.editor || name === this.current) return;
    this.states.set(this.current, this.editor.state);
    this.current = name;
    this.editor.setState(this.states.get(name)!);
    this.editor.dispatch({ effects: this.lock.reconfigure(EditorState.readOnly.of(this.busy)) });
    this.editor.focus();
    this.renderDirty = true;
    this.applyDiagnostics();
  }

  private async saveBuffers(): Promise<void> {
    for (const name of FILES) {
      const source = this.content(name);
      if (new TextEncoder().encode(source).length > MAX_SOURCE_BYTES) throw new Error(`${name} dépasse 256 Kio.`);
      if (source !== this.saved.get(name)) {
        this.view = await ide.save(name, source);
        this.saved.set(name, source);
      }
    }
  }

  async run(operation: "save" | "build" | "flash" | "both" | "restore"): Promise<void> {
    if (this.busy || !this.initialised) return;
    if (operation === "flash") {
      const refusal = flashRefusal(this.view, this.modified, this.board, this.checking);
      if (refusal) { this.error = refusal; this.renderDirty = true; return; }
    }
    this.operation = true;
    this.stateEpoch++;
    this.error = "";
    this.editor?.dispatch({ effects: this.lock.reconfigure(EditorState.readOnly.of(true)) });
    this.renderDirty = true;
    try {
      if (operation === "restore") {
        this.view = await ide.restore();
        for (const name of FILES) {
          const file = await ide.read(name);
          this.saved.set(name, file.content);
          this.states.set(name, EditorState.create({ doc: file.content, extensions: this.extensions() }));
        }
        this.editor?.setState(this.states.get(this.current)!);
        this.appendConsole("Template restauré. Compilez avant de programmer la carte.");
      } else {
        if (operation !== "flash") await this.saveBuffers();
        if (operation === "save") this.appendConsole("Les deux fichiers étudiants sont enregistrés.");
        if (operation === "build" || operation === "both") {
          this.appendConsole("Compilation de la source enregistrée…");
          this.view = await ide.build();
          this.showResult(this.view.build);
          if (!this.view.build?.ok) { this.error = failureText(this.view.build, "Compilation échouée. Corrigez les problèmes indiqués."); return; }
        }
        if (operation === "flash" || operation === "both") {
          const refusal = flashRefusal(this.view, this.modified, this.board, this.checking);
          if (refusal) throw new Error(refusal);
          this.appendConsole("Flash de la zone étudiante, puis reconnexion…");
          this.view = await ide.flash();
          this.showResult(this.view.flash);
          if (!this.view.flash?.ok) this.error = failureText(this.view.flash, "Programmation ou vérification échouée. Consultez la console.");
        }
      }
      this.applyDiagnostics();
    } catch (err) {
      this.error = String(err);
      this.appendConsole(this.error);
      try { this.view = await ide.state(); } catch { /* keep the useful error */ }
    } finally {
      this.operation = false;
      this.stateEpoch++;
      this.editor?.dispatch({ effects: this.lock.reconfigure(EditorState.readOnly.of(false)) });
      this.applyDiagnostics();
      this.renderDirty = true;
    }
  }

  private appendConsole(line: string): void {
    this.consoleLines.push(line);
    if (this.consoleLines.length > 200) this.consoleLines.splice(0, this.consoleLines.length - 200);
    this.renderDirty = true;
  }

  private showResult(result: ToolResult | null): void {
    if (!result) return;
    this.lastOperation = result.operation;
    this.appendConsole(result.message ?? (result.ok ? "Opération réussie." : "Opération échouée."));
    for (const d of result.diagnostics.slice(0, 100)) this.appendConsole(this.diagnosticText(d));
    if (result.artifact) this.appendConsole(`CRC étudiant ${crc(result.artifact.student_crc32)} · ${result.artifact.flash_bytes} octets de flash · ${result.artifact.ram_bytes} octets de RAM`);
  }

  private diagnosticText(d: Diagnostic): string {
    // a problem located in no file belongs to the operation that reported it (a missing tool, the ST-LINK)
    const where = d.file ?? (this.lastOperation === "flash" ? "Programmation" : "Compilation");
    return `${where}${d.line ? `:${d.line}${d.column ? `:${d.column}` : ""}` : ""} · ${d.severity} · ${d.message}`;
  }

  private applyDiagnostics(): void {
    if (!this.editor) return;
    const diagnostics = this.modified ? [] : this.view?.diagnostics ?? [];
    const source = this.content(this.current);
    this.editor.dispatch(setDiagnostics(this.editor.state, diagnostics.filter((d) => d.file === this.current && d.line).map((d) => {
      const from = compilerPositionOffset(source, d.line!, d.column ?? 1);
      return { from, to: Math.min(source.length, from + 1), severity: d.severity === "warning" ? "warning" as const : d.severity === "info" || d.severity === "note" ? "info" as const : "error" as const, message: d.message, source: "GCC" };
    })));
  }

  private goto(line: number, column = 1, compiler = false): void {
    if (!this.editor) return;
    const pos = (compiler ? compilerPositionOffset : positionOffset)(this.content(this.current), line, column);
    this.editor.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: "center" }) });
    this.editor.focus();
  }

  private render(): void {
    const busy = this.busy;
    const enabled = this.initialised && !!this.view?.available && !busy;
    const dirty = this.modified;
    const refusal = flashRefusal(this.view, dirty, this.board, this.checking);
    $<HTMLButtonElement>("#ide-save").disabled = !enabled || !dirty;
    $<HTMLButtonElement>("#ide-build").disabled = !enabled;
    $<HTMLButtonElement>("#ide-restore").disabled = !enabled;
    const flash = $<HTMLButtonElement>("#ide-flash");
    flash.disabled = !enabled || !!refusal;
    flash.title = refusal ?? "Programmer la compilation actuelle sur la carte connectée";
    const both = $<HTMLButtonElement>("#ide-build-flash");
    both.disabled = !enabled || !!flashRefusal(this.view ? { ...this.view, built_revision: this.view.revision, build: { ok: true } as ToolResult } : null, false, this.board, this.checking);
    both.title = "Enregistrer, compiler, programmer, puis vérifier la reconnexion";
    const message = $("#ide-message");
    setText(message, this.error || (this.view?.busy ? PHASE[this.view.phase] : busy ? "Opération en cours…" : !this.view?.available ? this.view?.reason ?? "Chargement du projet…" : dirty ? "Source modifiée — enregistrez et compilez avant le flash." : "Projet enregistré. Compiler → Flasher → Mission et résultats CAN."));
    message.classList.toggle("err", !!this.error);
    for (const b of Array.from(document.querySelectorAll<HTMLElement>('[data-view="code"] [data-file]'))) {
      const name = b.dataset.file as StudentFile;
      b.setAttribute(b.classList.contains("tab") ? "aria-selected" : "aria-pressed", String(name === this.current));
      const dot = b.querySelector<HTMLElement>(".ide-dirty");
      if (dot) dot.hidden = this.content(name) === this.saved.get(name);
    }
    if (this.editor) {
      const p = cursorPosition(this.content(this.current), this.editor.state.selection.main.head);
      setText($("#ide-cursor"), `Ln ${p.line}, Col ${p.column}`);
      setText($("#ide-source-version"), `Source r${this.view?.revision ?? 0}${dirty ? " · modifiée" : " · enregistrée"}`);
    }
    setText($("#ide-revision"), `r${this.view?.revision ?? 0}${dirty ? " + modifications" : ""}`);
    const artifact = this.view?.build?.artifact;
    setText($("#ide-flash-size"), artifact ? `${artifact.flash_bytes} octets` : "—");
    setText($("#ide-ram-size"), artifact ? `${artifact.ram_bytes} octets` : "—");
    setText($("#ide-build-crc"), artifact ? crc(artifact.student_crc32) : "—");
    setText($("#ide-board-state"), this.board?.state ?? "Déconnectée");
    setText($("#ide-runtime"), this.board?.board?.runtime ?? "—");
    setText($("#ide-integrity"), this.board?.board ? this.board.board.runtime_verified ? "Vérifiée" : "Non vérifiée" : "—");
    setText($("#ide-board-version"), this.board?.board?.student_abi ?? "—");
    setText($("#ide-board-crc"), this.board?.board?.student_crc32 ?? "—");
    const loaded = this.board?.board?.student_crc32;
    const matches = artifact && loaded ? parseInt(loaded.replace(/^0x/i, ""), 16) === artifact.student_crc32 : false;
    setText($("#ide-match"), loaded && artifact ? matches && !dirty && this.view?.built_revision === this.view?.revision ? "La carte exécute la compilation actuelle." : "Le code sur la carte diffère de la source actuelle." : "Connectez la carte pour vérifier le code chargé.");
    const key = JSON.stringify([this.current, this.view?.diagnostics, this.content(this.current), dirty]);
    if (key !== this.lastResultKey) {
      this.lastResultKey = key;
      this.renderDetails();
    }
    setText($("#ide-console"), this.consoleLines.join("\n"));
  }

  private renderDetails(): void {
    const outlineElement = $("#ide-outline");
    clear(outlineElement);
    for (const row of outline(this.content(this.current))) {
      const b = el("button", "ide-outline-item", `${row.name}()`);
      b.type = "button"; b.addEventListener("click", () => this.goto(row.line)); outlineElement.appendChild(b);
    }
    const panel = $("#ide-problems");
    clear(panel);
    const diagnostics = this.view?.diagnostics ?? [];
    setText($("#ide-problem-count"), String(diagnostics.length));
    if (!diagnostics.length) panel.appendChild(el("p", "empty", "Aucun diagnostic de compilation."));
    if (this.modified && diagnostics.length) panel.appendChild(el("p", "hint ide-note", "Diagnostics du dernier build. Recompilez la source modifiée."));
    for (const d of diagnostics.slice(0, 100)) {
      const b = el("button", `ide-problem ${d.severity === "warning" ? "warn" : "err"}`, this.diagnosticText(d));
      b.type = "button";
      b.disabled = !d.file || !FILES.includes(d.file as StudentFile) || !d.line;
      b.addEventListener("click", () => { this.openFile(d.file as StudentFile); this.goto(d.line!, d.column ?? 1, true); });
      panel.appendChild(b);
    }
    this.applyDiagnostics();
  }
}
