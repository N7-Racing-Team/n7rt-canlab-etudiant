import { describe, expect, it } from "vitest";
import catalogRs from "../../../../crates/n7rt-circuit/src/catalog.rs?raw";
import type { KindDef } from "../types";
import { LIBRARY } from "./testkit";

// The canvas tests run on the testkit library, transcribed from the core's catalog. This test reads the catalog
// source itself and checks the transcription field by field, so that the symbol, pointing and gesture oracles
// always describe the real library (the geometry the core sends with phys_library).

const unescape = (s: string): string => s.replace(/\\u\{([0-9a-fA-F]+)\}/g, (_, h: string) => String.fromCodePoint(parseInt(h, 16)));
const snake = (s: string): string => s.replace(/[A-Z]/g, (c, i: number) => (i ? "_" : "") + c.toLowerCase());

interface Call {
  name: string;
  args: string[];
  at: number;
}

/** Calls of the named functions in `text`, in order, with their top-level arguments as source text. */
function calls(text: string, names: string[]): Call[] {
  const out: Call[] = [];
  const re = new RegExp(`\\b(${names.join("|")})\\(`, "g");
  for (let m = re.exec(text); m; m = re.exec(text)) {
    const args: string[] = [];
    let depth = 0;
    let cur = "";
    let i = m.index + m[0].length;
    for (; i < text.length; i++) {
      const c = text[i];
      if (c === '"') {
        let j = i + 1;
        while (text[j] !== '"') j += text[j] === "\\" ? 2 : 1;
        cur += text.slice(i, j + 1);
        i = j;
      } else if (c === "(" || c === "[" || c === "{") {
        depth++;
        cur += c;
      } else if (c === ")" || c === "]" || c === "}") {
        if (depth === 0) break;
        depth--;
        cur += c;
      } else if (c === "," && depth === 0) {
        args.push(cur.trim());
        cur = "";
      } else {
        cur += c;
      }
    }
    if (cur.trim()) args.push(cur.trim());
    out.push({ name: m[1], args, at: m.index });
    re.lastIndex = i;
  }
  return out;
}

function parseCatalog(src: string): KindDef[] {
  const ohm = unescape(/const OHM: &str = "([^"]*)";/.exec(src)?.[1] ?? "");
  const str = (a: string): string => {
    if (a === "OHM") return ohm;
    const m = /^"((?:[^"\\]|\\.)*)"$/.exec(a);
    if (!m) throw new Error(`chaîne attendue : ${a}`);
    return unescape(m[1]);
  };
  const nums = (a: string): number[] => (a.match(/-?[\d.]+(?:e-?\d+)?/g) ?? []).map(Number);
  const rect = (a: string) => {
    const [x0, y0, x1, y1] = nums(a);
    return { x0, y0, x1, y1 };
  };
  const pinOf = (c: Call) => {
    const [x, y] = nums(c.args[3]);
    return {
      id: str(c.args[0]), short: str(c.args[2]), pos: [x, y], dir: c.args[4].replace("Dir::", "").toLowerCase(),
      role: snake(c.args[5].replace("Role::", "")), required: c.args[6] === "true",
    };
  };
  const section = (block: string, from: string, to: string): string => {
    const a = block.indexOf(from);
    const b = block.indexOf(to, a);
    if (a < 0 || b < 0) throw new Error(`section ${from} introuvable`);
    return block.slice(a + from.length, b);
  };
  const two = calls(/const TWO_TERMINAL[^=]*=\s*\[([\s\S]*?)\];/.exec(src)?.[1] ?? "", ["pin"]).map(pinOf);
  const lib = src.slice(src.indexOf("pub static LIBRARY"));
  return lib.split(/\n\s*KindDef \{/).slice(1).map((block): KindDef => {
    const pinsText = section(block, "pins:", "params:");
    const params = calls(section(block, "params:", "states:"), ["num_or_inf", "num", "fixed", "int", "boolean", "choice"]).map((c) => {
      const name = str(c.args[0]);
      switch (c.name) {
        case "num": {
          const [min, max, def] = c.args.slice(3).map(Number);
          return { name, unit: str(c.args[2]), vtype: "number", min, max, default: def, choices: [], editable: true };
        }
        case "fixed": {
          const v = Number(c.args[3]);
          return { name, unit: str(c.args[2]), vtype: "number", min: v, max: v, default: v, choices: [], editable: false };
        }
        case "int": {
          const [min, max, def] = c.args.slice(3).map(Number);
          return { name, unit: str(c.args[2]), vtype: "int", min, max, default: def, choices: [], editable: true };
        }
        case "num_or_inf":
          return { name, unit: str(c.args[2]), vtype: "number_or_inf", min: Number(c.args[3]), max: Number(c.args[4]), default: "inf", choices: [], editable: true };
        case "boolean":
          return { name, unit: "", vtype: "bool", min: 0, max: 1, default: c.args[2] === "true", choices: [], editable: true };
        default:
          return {
            name, unit: "", vtype: "choice", min: 0, max: 0, default: str(c.args[3]),
            choices: (c.args[2].match(/"[^"]*"/g) ?? []).map(str), editable: true,
          };
      }
    });
    const statesText = section(block, "states:", "actuators:");
    const states = [
      ...calls(statesText, ["state_bool", "state_transient"]).map((c) => ({
        at: c.at, name: str(c.args[0]), default: c.name === "state_bool" ? c.args[2] === "true" : false, persistent: c.name === "state_bool",
      })),
      ...[...statesText.matchAll(/StateDef \{\s*name: "(\w+)",[\s\S]*?default: Lit::\w+\(([^)]*)\),\s*persistent: (true|false)/g)].map((m) => ({
        at: m.index ?? 0, name: m[1], default: Number(m[2]), persistent: m[3] === "true",
      })),
    ].sort((a, b) => a.at - b.at).map(({ name, default: d, persistent }) => ({ name, default: d, persistent }));
    const actuators = [...section(block, "actuators:", "placeable:").matchAll(/ActuatorKind::(\w+), zone: (rect\([^)]*\))/g)]
      .map((m) => ({ kind: snake(m[1]), zone: rect(m[2]) }));
    return {
      kind: snake(/kind: Kind::(\w+)/.exec(block)?.[1] ?? ""),
      prefix: /prefix: "([^"]*)"/.exec(block)?.[1] ?? "",
      body: rect(/body: (rect\([^)]*\))/.exec(block)?.[1] ?? ""),
      pins: pinsText.includes("&TWO_TERMINAL") ? two : calls(pinsText, ["pin"]).map(pinOf),
      params, states, actuators,
      placeable: /placeable: true/.test(block),
    } as unknown as KindDef;
  });
}

/** The fields the canvas and its tests rely on (labels, full pin names and sources are not drawn). */
function essentials(d: KindDef) {
  return {
    kind: d.kind,
    prefix: d.prefix,
    body: d.body,
    pins: d.pins.map((p) => ({ id: p.id, short: p.short, pos: p.pos, dir: p.dir, role: p.role, required: p.required })),
    params: d.params.map((p) => ({
      name: p.name, unit: p.unit, vtype: p.vtype, min: p.min, max: p.max, default: p.default, choices: p.choices, editable: p.editable,
    })),
    states: d.states.map((s) => ({ name: s.name, default: s.default, persistent: s.persistent })),
    actuators: d.actuators,
    placeable: d.placeable,
  };
}

describe("test library", () => {
  const core = parseCatalog(catalogRs);

  it("reads the 15 kinds of the core catalog, each with its pins", () => {
    expect(core).toHaveLength(15);
    for (const d of core) expect(d.pins.length, d.kind).toBeGreaterThan(0);
    expect(core.find((d) => d.kind === "potentiometer")?.actuators).toEqual([{ kind: "drag_pot", zone: { x0: -2, y0: -2, x1: 2, y1: -1 } }]);
  });

  it("is the core catalog: kinds in order, bodies, pins, parameters, states and actuators", () => {
    expect(LIBRARY.map((d) => d.kind)).toEqual(core.map((d) => d.kind));
    for (const d of core) {
      const t = LIBRARY.find((k) => k.kind === d.kind);
      expect(t && essentials(t), d.kind).toEqual(essentials(d));
    }
  });
});
