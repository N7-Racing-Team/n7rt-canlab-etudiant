//! n7rt-tool: instructor console tool of N7RT CAN Lab (docs/distribution.md §4). Every command is a library function
//! taking its code source and its output, so tests run without a terminal. The instructor code is read without echo
//! (or from stdin with `--code-stdin`), never from the command line, and is never printed or written. Group codes are
//! the instructor's to hand out: gen-packs/test-pack print them and keep them in the instructor directory only.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use n7rt_seal::{
    group_seed, hash_code, new_group_code, new_master_key, normalize_code, random_bytes, read_pack_header, seal_pack,
    sha256_hex, verify_code, PackHeader,
};
use n7rt_vehicle::{judge_run, Pack, RunRecord};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const EXIT_OK: i32 = 0;
/// verify-code: wrong code; judge: run not passed.
pub const EXIT_NO: i32 = 1;
pub const EXIT_ERROR: i32 = 2;

/// Minimum length of the instructor code, after normalization.
pub const MIN_INSTRUCTOR_CODE: usize = 12;
/// `judge` in a build without the oracle (docs/distribution.md §1).
pub const ORACLE_ABSENT: &str = "cet n7rt-tool ne contient pas l'oracle : jugez avec celui de l'archive des outils \
                                 encadrant (n7rt-tool --version affiche « édition encadrant »)";
pub const DEFAULT_TEST_COHORT: &str = "essais";
/// Largest run file read by `judge` and `inspect` (bytes), refused before it is read. A nominal APPS run is a few
/// hundred KiB.
pub const MAX_RUN_BYTES: u64 = 8 << 20;
/// Largest group pack (bytes), refused before it is read. A release 0.2 pack is a few tens of KiB; a release 0.3 pack
/// carries the sealed hidden tests of its steps (about 1–2 MiB for a pool of two), hence the bound of the run files.
pub const MAX_PACK_BYTES: u64 = 8 << 20;

pub const USAGE: &str = "\
n7rt-tool — outil encadrant de N7RT CAN Lab

Utilisation :
  n7rt-tool set-code <dossier encadrant>
      Définit ou change le code encadrant (saisie masquée, deux fois ; 12 caractères au moins, espaces et
      tirets non comptés ; le code actuel est demandé d'abord s'il existe). Crée instructor/master.key
      s'il manque, sans jamais le remplacer.
  n7rt-tool verify-code <dossier encadrant>
      Vérifie le code encadrant (code de sortie 0 s'il est correct, 1 sinon).
  n7rt-tool gen-packs <dossier encadrant> --mission <mission> --cohort <cohorte> --groups G01,G02,… [--replace]
      Génère pour chaque groupe sa variante, son code et son pack chiffré :
      cohorts/<cohorte>/<mission>/packs/<groupe>.n7rtpack et cohorts/<cohorte>/<mission>/groups.json.
      La casse ne distingue pas deux groupes (g01 = G01) : un nouveau groupe est écrit en majuscules, un
      groupe déjà présent garde son nom enregistré.
  n7rt-tool test-pack <dossier encadrant> --mission <mission> --group <groupe> [--cohort essais] [--replace]
      Pack chiffré de la variante V0 (référence), pour un essai encadrant avec le décodeur de référence.
  n7rt-tool judge <dossier encadrant> <fichier .n7rtrun>
      Juge le fichier d'essai d'un groupe : verdict, détail privé, cohorts/<c>/<m>/judged/<essai>.json
      (<essai>-2.json, <essai>-3.json… si ce nom est pris : un jugement n'est jamais remplacé).
      Demande l'n7rt-tool de l'archive des outils encadrant (avec l'oracle, voir --version).
  n7rt-tool inspect <fichier .n7rtpack | .n7rtrun>
      En-tête d'un pack ou résumé d'un fichier d'essai (aucun secret, aucun code demandé).

Options :
  --code-stdin   lit les codes sur l'entrée standard, une ligne par code demandé et dans l'ordre des
                 questions, au lieu de la saisie masquée (automatisation).
                 Un code n'est jamais accepté sur la ligne de commande.
  --replace      régénère les groupes déjà présents (nouveau code et nouveau pack à redistribuer).

Les commandes qui lisent le dossier encadrant demandent le code encadrant.
Codes de sortie : 0 succès ; 1 code incorrect (verify-code) ou essai non réussi (judge) ; 2 erreur.
";

// ---------------------------------------------------------------------------------------------------- errors

/// Error of a command: a French message that never contains a code or a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError(pub String);

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ToolError {}

fn fail<T>(msg: impl Into<String>) -> Result<T, ToolError> {
    Err(ToolError(msg.into()))
}

fn io_fail<'a>(what: &'a str, path: &'a Path) -> impl FnOnce(io::Error) -> ToolError + 'a {
    move |e| ToolError(format!("{what} {} : {e}", path.display()))
}

// ------------------------------------------------------------------------------------------------ code input

/// Where codes come from: the console without echo, or one line of an input stream per requested code.
pub trait CodeSource {
    fn read_code(&mut self, prompt: &str) -> Result<String, ToolError>;
}

/// Masked console entry (rpassword).
pub struct TtyCodes;

impl CodeSource for TtyCodes {
    fn read_code(&mut self, prompt: &str) -> Result<String, ToolError> {
        rpassword::prompt_password(prompt)
            .map_err(|e| ToolError(format!("saisie masquée impossible ({e}) : utilisez --code-stdin")))
    }
}

/// One code per line (`--code-stdin`, tests). The prompt is not shown.
pub struct LineCodes<R: BufRead>(pub R);

impl<R: BufRead> CodeSource for LineCodes<R> {
    fn read_code(&mut self, _prompt: &str) -> Result<String, ToolError> {
        let mut line = String::new();
        let n = self.0.read_line(&mut line).map_err(|e| ToolError(format!("lecture de l'entrée standard : {e}")))?;
        if n == 0 {
            return fail("entrée standard terminée : un code était attendu");
        }
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        Ok(line)
    }
}

// ----------------------------------------------------------------------------------------------- arguments

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    SetCode { dir: PathBuf },
    VerifyCode { dir: PathBuf },
    GenPacks { dir: PathBuf, mission: String, cohort: String, groups: Vec<String>, replace: bool },
    TestPack { dir: PathBuf, mission: String, cohort: String, group: String, replace: bool },
    Judge { dir: PathBuf, run: PathBuf },
    Inspect { file: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub command: Command,
    /// Read codes from stdin instead of the masked console entry.
    pub code_stdin: bool,
}

const NO_CODE_ARG: &str =
    "un code ne se passe jamais sur la ligne de commande : saisie masquée, ou --code-stdin pour l'automatisation";

/// Parses the arguments (without the program name). Error messages never repeat an argument value.
pub fn parse_args(args: &[String]) -> Result<Cli, ToolError> {
    let Some(cmd) = args.first() else { return fail("commande manquante (n7rt-tool --help)") };
    let simple = |command: Command| -> Result<Cli, ToolError> { Ok(Cli { command, code_stdin: false }) };
    match cmd.as_str() {
        "-h" | "--help" | "help" | "aide" => return simple(Command::Help),
        "-V" | "--version" | "version" => return simple(Command::Version),
        _ => {}
    }
    let mut positional: Vec<&String> = Vec::new();
    let mut values: BTreeMap<&str, String> = BTreeMap::new();
    let mut flags: Vec<&str> = Vec::new();
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        if let Some(opt) = a.strip_prefix("--") {
            let (name, inline) = match opt.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (opt, None),
            };
            if name != "code-stdin" && (name.contains("code") || name.contains("pass") || name.contains("mot")) {
                return fail(NO_CODE_ARG);
            }
            match name {
                "replace" | "code-stdin" => {
                    if inline.is_some() {
                        return fail(format!("--{name} ne prend pas de valeur"));
                    }
                    if !flags.contains(&name) {
                        flags.push(name);
                    }
                }
                "mission" | "cohort" | "group" | "groups" => {
                    let v = match inline {
                        Some(v) => v.to_string(),
                        None => match it.next() {
                            Some(v) if !v.starts_with("--") => v.clone(),
                            _ => return fail(format!("valeur manquante après --{name}")),
                        },
                    };
                    if values.insert(name, v).is_some() {
                        return fail(format!("option --{name} répétée"));
                    }
                }
                "help" => return simple(Command::Help),
                _ => return fail(format!("option inconnue : --{name}")),
            }
        } else if a.starts_with('-') && a.len() > 1 {
            return fail("option inconnue (les options commencent par --)");
        } else {
            positional.push(a);
        }
    }

    let (n_pos, allowed_values, allowed_flags): (usize, &[&str], &[&str]) = match cmd.as_str() {
        "set-code" | "verify-code" => (1, &[], &["code-stdin"]),
        "gen-packs" => (1, &["mission", "cohort", "groups"], &["code-stdin", "replace"]),
        "test-pack" => (1, &["mission", "cohort", "group"], &["code-stdin", "replace"]),
        "judge" => (2, &[], &["code-stdin"]),
        "inspect" => (1, &[], &[]),
        _ => return fail("commande inconnue (n7rt-tool --help)"),
    };
    for k in values.keys() {
        if !allowed_values.contains(k) {
            return fail(format!("{cmd} : option --{k} non prise en charge"));
        }
    }
    for f in &flags {
        if !allowed_flags.contains(f) {
            return fail(format!("{cmd} : option --{f} non prise en charge"));
        }
    }
    if positional.len() < n_pos {
        let what = match cmd.as_str() {
            "inspect" => "fichier .n7rtpack ou .n7rtrun attendu",
            "judge" if positional.len() == 1 => "fichier d'essai .n7rtrun attendu après le dossier encadrant",
            _ => "dossier encadrant attendu",
        };
        return fail(format!("{cmd} : {what}"));
    }
    if positional.len() > n_pos {
        return fail(format!("{cmd} : argument en trop"));
    }
    let mut take = |k: &str| -> Result<String, ToolError> {
        values.remove(k).ok_or_else(|| ToolError(format!("{cmd} : option --{k} obligatoire")))
    };
    let p0 = PathBuf::from(positional[0]);
    let command = match cmd.as_str() {
        "set-code" => Command::SetCode { dir: p0 },
        "verify-code" => Command::VerifyCode { dir: p0 },
        "gen-packs" => {
            let mission = take("mission")?;
            let cohort = take("cohort")?;
            let groups: Vec<String> =
                take("groups")?.split(',').map(str::trim).filter(|g| !g.is_empty()).map(String::from).collect();
            if groups.is_empty() {
                return fail("gen-packs : liste --groups vide");
            }
            Command::GenPacks { dir: p0, mission, cohort, groups, replace: flags.contains(&"replace") }
        }
        "test-pack" => {
            let mission = take("mission")?;
            let group = take("group")?;
            let cohort = take("cohort").unwrap_or_else(|_| DEFAULT_TEST_COHORT.to_string());
            Command::TestPack { dir: p0, mission, cohort, group, replace: flags.contains(&"replace") }
        }
        "judge" => Command::Judge { dir: p0, run: PathBuf::from(positional[1]) },
        _ => Command::Inspect { file: p0 },
    };
    Ok(Cli { command, code_stdin: flags.contains(&"code-stdin") })
}

/// Runs a parsed command; Ok(exit code) or a command error (exit code [`EXIT_ERROR`]).
pub fn run(cli: &Cli, codes: &mut dyn CodeSource, out: &mut dyn Write) -> Result<i32, ToolError> {
    run_with(cli, codes, out, None)
}

/// [`run`] whose generated packs take their sealed content from `hook` (release 0.3, instructor edition).
pub fn run_with(cli: &Cli, codes: &mut dyn CodeSource, out: &mut dyn Write, hook: Option<ContentHook>) -> Result<i32, ToolError> {
    match &cli.command {
        Command::Help => {
            say(out, USAGE.trim_end());
            Ok(EXIT_OK)
        }
        Command::Version => {
            say(out, format!("n7rt-tool {} ({})", env!("CARGO_PKG_VERSION"), n7rt_vehicle::EDITION));
            Ok(EXIT_OK)
        }
        Command::SetCode { dir } => set_code(dir, codes, out).map(|_| EXIT_OK),
        Command::VerifyCode { dir } => {
            if check_instructor_code(dir, codes)? {
                say(out, "Code encadrant correct.");
                Ok(EXIT_OK)
            } else {
                say(out, "Code encadrant incorrect.");
                Ok(EXIT_NO)
            }
        }
        Command::GenPacks { dir, mission, cohort, groups, replace } => {
            gen_packs_with(dir, mission, cohort, groups, *replace, codes, out, hook).map(|_| EXIT_OK)
        }
        Command::TestPack { dir, mission, cohort, group, replace } => {
            test_pack_with(dir, mission, cohort, group, *replace, codes, out, hook).map(|_| EXIT_OK)
        }
        Command::Judge { dir, run } => judge(dir, run, codes, out).map(|j| if j.pass { EXIT_OK } else { EXIT_NO }),
        Command::Inspect { file } => inspect(file, out).map(|_| EXIT_OK),
    }
}

fn say(out: &mut dyn Write, line: impl fmt::Display) {
    let _ = writeln!(out, "{line}");
}

// ------------------------------------------------------------------------------------------ instructor files

const AUTH_FORMAT: &str = "n7rt-instructor-auth";

/// `instructor/auth.json`.
#[derive(Debug, Serialize, Deserialize)]
struct AuthFile {
    format: String,
    version: u32,
    code_phc: String,
}

fn auth_path(root: &Path) -> PathBuf {
    root.join("instructor").join("auth.json")
}

fn master_path(root: &Path) -> PathBuf {
    root.join("instructor").join("master.key")
}

fn pack_v0_path(root: &Path, mission: &str) -> PathBuf {
    root.join("missions").join(mission).join("pack-v0.json")
}

fn cohort_dir(root: &Path, cohort: &str, mission: &str) -> PathBuf {
    root.join("cohorts").join(cohort).join(mission)
}

fn groups_path(root: &Path, cohort: &str, mission: &str) -> PathBuf {
    cohort_dir(root, cohort, mission).join("groups.json")
}

fn check_dir(root: &Path) -> Result<(), ToolError> {
    if root.is_dir() {
        Ok(())
    } else {
        fail(format!("dossier encadrant introuvable : {}", root.display()))
    }
}

/// Writes through a temporary file then renames, so a reader never sees a half-written file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ToolError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_fail("création du dossier", parent))?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.tmp-{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(io_fail("écriture de", &tmp))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        ToolError(format!("écriture de {} : {e}", path.display()))
    })
}

/// Writes `bytes` as a new file of `dir`: `<stem>.<ext>`, else `<stem>-2.<ext>`, `<stem>-3.<ext>`… (the scheme of the
/// student's run files). An existing file is never replaced: the name is taken by an exclusive creation, then filled
/// through [`write_atomic`].
fn write_new(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf, ToolError> {
    fs::create_dir_all(dir).map_err(io_fail("création du dossier", dir))?;
    for k in 1..1000 {
        let path = if k == 1 { dir.join(format!("{stem}.{ext}")) } else { dir.join(format!("{stem}-{k}.{ext}")) };
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                drop(file);
                return match write_atomic(&path, bytes) {
                    Ok(()) => Ok(path),
                    Err(e) => {
                        let _ = fs::remove_file(&path);
                        Err(e)
                    }
                };
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return fail(format!("création de {} : {e}", path.display())),
        }
    }
    fail(format!("aucun nom libre pour {stem}.{ext} dans {}", dir.display()))
}

/// Reads a file of at most `max` bytes; a larger one is refused before it is read.
fn read_bounded(path: &Path, max: u64, what: &str) -> Result<Vec<u8>, ToolError> {
    let file = fs::File::open(path).map_err(io_fail("lecture de", path))?;
    let too_large = || ToolError(format!("{what} trop volumineux : {} Mio au plus", max >> 20));
    if file.metadata().map_err(io_fail("lecture de", path))?.len() > max {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes).map_err(io_fail("lecture de", path))?;
    if bytes.len() as u64 > max {
        return Err(too_large());
    }
    Ok(bytes)
}

/// A text of a file shown as it is: printable ASCII of 64 characters at most, else « ? ».
fn shown(v: &Value) -> &str {
    v.as_str().filter(|s| s.len() <= 64 && s.bytes().all(|b| (0x20..=0x7e).contains(&b))).unwrap_or("?")
}

fn read_auth(root: &Path) -> Result<AuthFile, ToolError> {
    check_dir(root)?;
    let path = auth_path(root);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return fail("code encadrant non défini (instructor/auth.json absent) : lancez « n7rt-tool set-code »")
        }
        Err(e) => return fail(format!("lecture de {} : {e}", path.display())),
    };
    let auth: AuthFile =
        serde_json::from_str(&text).map_err(|_| ToolError("instructor/auth.json illisible".into()))?;
    if auth.format != AUTH_FORMAT || auth.version != 1 {
        return fail("instructor/auth.json : format ou version non pris en charge");
    }
    Ok(auth)
}

/// Asks the instructor code and checks it against `instructor/auth.json`: Ok(false) for a wrong code.
pub fn check_instructor_code(root: &Path, codes: &mut dyn CodeSource) -> Result<bool, ToolError> {
    let auth = read_auth(root)?;
    let code = codes.read_code("Code encadrant : ")?;
    Ok(verify_code(&auth.code_phc, &code))
}

fn authenticate(root: &Path, codes: &mut dyn CodeSource) -> Result<(), ToolError> {
    if check_instructor_code(root, codes)? {
        Ok(())
    } else {
        fail("code encadrant incorrect")
    }
}

fn read_master(root: &Path) -> Result<[u8; 32], ToolError> {
    let path = master_path(root);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return fail("clé maîtresse absente (instructor/master.key) : lancez « n7rt-tool set-code »")
        }
        Err(e) => return fail(format!("lecture de {} : {e}", path.display())),
    };
    parse_hex32(text.trim()).ok_or_else(|| ToolError("instructor/master.key invalide (64 caractères hexadécimaux attendus)".into()))
}

fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(k)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Mission, cohort and group names become path components: letters, digits, `_`, `-`, 32 characters at most.
pub fn check_name(kind: &str, v: &str) -> Result<(), ToolError> {
    let ok = !v.is_empty()
        && v.len() <= 32
        && v.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        && v.as_bytes()[0].is_ascii_alphanumeric();
    if ok {
        Ok(())
    } else {
        fail(format!("{kind} « {v} » invalide : lettres, chiffres, « _ » et « - » seulement, 32 caractères au plus"))
    }
}

// ------------------------------------------------------------------------------------------------- set-code

/// `set-code`: asks the current code when one is set, then the new code twice; writes `instructor/auth.json`;
/// creates `instructor/master.key` when absent (never overwrites it).
pub fn set_code(root: &Path, codes: &mut dyn CodeSource, out: &mut dyn Write) -> Result<(), ToolError> {
    check_dir(root)?;
    let auth = auth_path(root);
    if auth.exists() {
        let current = read_auth(root)?;
        let code = codes.read_code("Code encadrant actuel : ")?;
        if !verify_code(&current.code_phc, &code) {
            return fail("code encadrant actuel incorrect");
        }
    }
    let new = codes.read_code("Nouveau code encadrant : ")?;
    let confirm = codes.read_code("Confirmez le nouveau code : ")?;
    let norm = normalize_code(&new);
    if norm != normalize_code(&confirm) {
        return fail("les deux saisies diffèrent : code inchangé");
    }
    if norm.chars().count() < MIN_INSTRUCTOR_CODE {
        return fail(format!(
            "le code encadrant doit compter au moins {MIN_INSTRUCTOR_CODE} caractères (espaces et tirets non comptés)"
        ));
    }
    let phc = hash_code(&new).map_err(|e| ToolError(format!("hachage du code : {e}")))?;
    let file = AuthFile { format: AUTH_FORMAT.into(), version: 1, code_phc: phc };
    let mut text = serde_json::to_string_pretty(&file).expect("serializable");
    text.push('\n');
    write_atomic(&auth, text.as_bytes())?;
    say(out, "Code encadrant enregistré (instructor/auth.json).");

    let master = master_path(root);
    match fs::OpenOptions::new().write(true).create_new(true).open(&master) {
        Ok(mut f) => {
            f.write_all(hex(&new_master_key()).as_bytes()).map_err(io_fail("écriture de", &master))?;
            say(out, "Clé maîtresse créée (instructor/master.key) : elle fixe les variantes de tous les groupes.");
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            read_master(root)?;
            say(out, "Clé maîtresse existante conservée (instructor/master.key).");
        }
        Err(e) => return fail(format!("création de {} : {e}", master.display())),
    }
    say(out, "Ne diffusez jamais le dossier instructor/.");
    Ok(())
}

// ----------------------------------------------------------------------------------------------- groups.json

/// One entry of `cohorts/<cohort>/<mission>/groups.json`. Unknown fields are kept.
#[derive(Clone, Serialize, Deserialize)]
pub struct GroupEntry {
    pub group: String,
    pub pack_id: String,
    pub code: String,
    pub created_utc: String,
    /// "v0" for an instructor test pack (reference variant); absent for a group variant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// pack_id of the packs this entry replaced (same variant: their run files are still judged).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_pack_ids: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

pub fn load_groups(path: &Path) -> Result<Vec<GroupEntry>, ToolError> {
    match fs::read_to_string(path) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| ToolError(format!("{} illisible : {e}", path.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => fail(format!("lecture de {} : {e}", path.display())),
    }
}

fn save_groups(path: &Path, entries: &[GroupEntry]) -> Result<(), ToolError> {
    let mut text = serde_json::to_string_pretty(entries).expect("serializable");
    text.push('\n');
    write_atomic(path, text.as_bytes())
}

/// A generated group pack, as printed in the table.
pub struct Generated {
    pub group: String,
    pub code: String,
    pub pack_id: String,
    pub file: PathBuf,
}

/// Debug output leaves the group code out.
impl fmt::Debug for Generated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Generated").field("group", &self.group).field("pack_id", &self.pack_id).field("file", &self.file).finish()
    }
}

struct Target<'a> {
    root: &'a Path,
    mission: &'a str,
    cohort: &'a str,
}

/// Seals one student pack under a new code, writes `packs/<group>.n7rtpack` and records it in groups.json.
fn write_group_pack(
    t: &Target,
    group: &str,
    student_json: &str,
    variant: Option<&str>,
    entries: &mut Vec<GroupEntry>,
) -> Result<Generated, ToolError> {
    let code = new_group_code();
    let pack_id = hex(&random_bytes(16));
    let header = PackHeader {
        mission: t.mission.into(),
        cohort: t.cohort.into(),
        group: group.into(),
        pack_id: pack_id.clone(),
    };
    let sealed = seal_pack(&header, student_json.as_bytes(), &code)
        .map_err(|e| ToolError(format!("scellement du pack du groupe {group} : {e}")))?;
    // the student import refuses a larger file: never write a pack that cannot be opened
    if sealed.len() as u64 > MAX_PACK_BYTES {
        return fail(format!("pack du groupe {group} trop volumineux : {} octets pour {} au plus", sealed.len(), MAX_PACK_BYTES));
    }
    let file = cohort_dir(t.root, t.cohort, t.mission).join("packs").join(format!("{group}.n7rtpack"));
    write_atomic(&file, &sealed)?;

    let mut entry = GroupEntry {
        group: group.into(),
        pack_id: pack_id.clone(),
        code: code.clone(),
        created_utc: utc_now(),
        variant: variant.map(String::from),
        previous_pack_ids: Vec::new(),
        extra: Map::new(),
    };
    match entries.iter().position(|e| e.group.eq_ignore_ascii_case(group)) {
        Some(i) => {
            let old = entries.remove(i);
            if old.variant == entry.variant {
                entry.previous_pack_ids = old.previous_pack_ids;
                entry.previous_pack_ids.push(old.pack_id);
            }
            entry.extra = old.extra;
            entries.insert(i, entry);
        }
        None => entries.push(entry),
    }
    save_groups(&groups_path(t.root, t.cohort, t.mission), entries)?;
    Ok(Generated { group: group.into(), code, pack_id, file })
}

fn load_reference_pack(root: &Path, mission: &str) -> Result<Pack, ToolError> {
    let path = pack_v0_path(root, mission);
    if !path.is_file() {
        return fail(format!("pack de référence absent : missions/{mission}/pack-v0.json"));
    }
    let pack = Pack::load(&path).map_err(|e| ToolError(format!("pack de référence missions/{mission}/pack-v0.json : {e}")))?;
    if pack.mission() != mission {
        return fail(format!("missions/{mission}/pack-v0.json décrit une autre mission"));
    }
    Ok(pack)
}

/// The name under which a typed group is generated. Letter case never tells two groups apart (`g01` is `G01`): a group
/// already in groups.json keeps its recorded spelling, a new group is written in upper case. The variant is seeded
/// from that name, so a replaced group keeps its variant and the run files of its earlier packs are still judged
/// against it (the judge seeds from the recorded name too).
fn group_name(entries: &[GroupEntry], typed: &str) -> String {
    entries.iter().find(|e| e.group.eq_ignore_ascii_case(typed)).map_or_else(|| typed.to_ascii_uppercase(), |e| e.group.clone())
}

/// The name under which a typed cohort is written. On Windows `cohorts/Promo` and `cohorts/promo` are the same folder:
/// an existing cohort folder that matches without regard to case keeps its spelling (seed, pack header, paths and
/// judgement all use it), a new cohort keeps the typed name.
fn cohort_name(root: &Path, typed: &str) -> String {
    fs::read_dir(root.join("cohorts"))
        .ok()
        .and_then(|dirs| {
            dirs.filter_map(Result::ok)
                .filter(|d| d.path().is_dir())
                .map(|d| d.file_name().to_string_lossy().into_owned())
                .find(|n| n.eq_ignore_ascii_case(typed))
        })
        .unwrap_or_else(|| typed.to_owned())
}

fn refuse_existing(entries: &[GroupEntry], groups: &[String], replace: bool) -> Result<(), ToolError> {
    if replace {
        return Ok(());
    }
    let existing: Vec<&str> = groups
        .iter()
        .filter_map(|g| entries.iter().find(|e| e.group.eq_ignore_ascii_case(g)))
        .map(|e| e.group.as_str())
        .collect();
    if existing.is_empty() {
        Ok(())
    } else {
        fail(format!(
            "groupe(s) déjà généré(s) : {} (--replace pour les régénérer avec un nouveau code)",
            existing.join(", ")
        ))
    }
}

fn print_table(out: &mut dyn Write, root: &Path, generated: &[Generated]) {
    let w = generated.iter().map(|g| g.group.chars().count()).max().unwrap_or(0).max("Groupe".len());
    say(out, format!("{:<w$}  {:<19}  Fichier", "Groupe", "Code"));
    for g in generated {
        let rel = g.file.strip_prefix(root).unwrap_or(&g.file);
        say(out, format!("{:<w$}  {:<19}  {}", g.group, g.code, rel.display()));
    }
}

/// Release 0.3: the sealed plaintext of a group, given the instructor directory, the group's full variant pack and its
/// 32-byte seed. `Ok(None)` keeps the 0.2 content (the student pack alone). The application's instructor edition
/// provides it (`n7rt_lab::workflow`): this library cannot depend on the physics, which depends on it.
pub type ContentHook<'a> = &'a dyn Fn(&Path, &Pack, &[u8; 32]) -> Result<Option<String>, String>;

fn plaintext(root: &Path, group: &str, variant: &Pack, seed: &[u8; 32], hook: Option<ContentHook>) -> Result<String, ToolError> {
    match hook {
        // a mission with a release 0.3 truth file never gets a 0.2 pack silently: its packs need the instructor edition
        None if root.join("missions").join(variant.mission()).join("workflow-v0.json").is_file() => fail(format!(
            "la mission {} a une progression 0.3 (missions/{}/workflow-v0.json) : générez ses packs avec n7rt-pack ou l'application encadrant",
            variant.mission(),
            variant.mission()
        )),
        None => Ok(variant.student_json()),
        Some(h) => match h(root, variant, seed) {
            Ok(Some(text)) => Ok(text),
            Ok(None) => Ok(variant.student_json()),
            Err(e) => fail(format!("contenu du pack du groupe {group} : {e}")),
        },
    }
}

/// `gen-packs`: one variant, code and sealed student pack per group (seed = HMAC(master key, mission, cohort, group),
/// the group under its recorded or upper-case name: [`group_name`], the cohort under its folder's spelling:
/// [`cohort_name`]).
pub fn gen_packs(
    root: &Path,
    mission: &str,
    cohort: &str,
    groups: &[String],
    replace: bool,
    codes: &mut dyn CodeSource,
    out: &mut dyn Write,
) -> Result<Vec<Generated>, ToolError> {
    gen_packs_with(root, mission, cohort, groups, replace, codes, out, None)
}

/// [`gen_packs`] whose sealed content comes from `hook` (release 0.3 packs).
#[allow(clippy::too_many_arguments)]
pub fn gen_packs_with(
    root: &Path,
    mission: &str,
    cohort: &str,
    groups: &[String],
    replace: bool,
    codes: &mut dyn CodeSource,
    out: &mut dyn Write,
    hook: Option<ContentHook>,
) -> Result<Vec<Generated>, ToolError> {
    check_name("mission", mission)?;
    check_name("cohorte", cohort)?;
    if groups.is_empty() {
        return fail("aucun groupe demandé");
    }
    for (i, g) in groups.iter().enumerate() {
        check_name("groupe", g)?;
        if groups[..i].iter().any(|h| h.eq_ignore_ascii_case(g)) {
            return fail(format!("groupe {g} demandé deux fois"));
        }
    }
    authenticate(root, codes)?;
    let master = read_master(root)?;
    let reference = load_reference_pack(root, mission)?;
    let cohort = cohort_name(root, cohort);
    let cohort = cohort.as_str();
    let gpath = groups_path(root, cohort, mission);
    let mut entries = load_groups(&gpath)?;
    refuse_existing(&entries, groups, replace)?;

    let t = Target { root, mission, cohort };
    let mut generated = Vec::with_capacity(groups.len());
    for typed in groups {
        let g = group_name(&entries, typed);
        let seed = group_seed(&master, mission, cohort, &g);
        let variant = reference.variant(&seed).map_err(|e| ToolError(format!("variante du groupe {g} : {e}")))?;
        let content = plaintext(root, &g, &variant, &seed, hook)?;
        generated.push(write_group_pack(&t, &g, &content, None, &mut entries)?);
    }
    say(out, format!("Cohorte {cohort}, mission {mission} : {} pack(s) généré(s).", generated.len()));
    print_table(out, root, &generated);
    say(out, format!("Codes enregistrés dans {} (dossier encadrant uniquement).", rel_display(root, &gpath)));
    Ok(generated)
}

/// The sealed content of the group that owns `pack_id`, regenerated from the master key (never read from a pack
/// file): what an export of that group is checked against.
pub struct GroupContent {
    pub cohort: String,
    pub group: String,
    /// `pack_id` is the group's current pack (not one replaced since by `--replace`).
    pub current: bool,
    pub text: String,
}

/// [`GroupContent`] of the group of `mission` whose current or previous pack is `pack_id`; `None` when no group of
/// the instructor directory has it.
pub fn group_content_of(root: &Path, mission: &str, pack_id: &str, codes: &mut dyn CodeSource, hook: ContentHook) -> Result<Option<GroupContent>, ToolError> {
    check_name("mission", mission)?;
    authenticate(root, codes)?;
    let Some((cohort, entry)) = find_pack(root, mission, pack_id)? else {
        return Ok(None);
    };
    let reference = load_reference_pack(root, mission)?;
    let (variant, seed) = if entry.variant.as_deref() == Some("v0") {
        (reference, test_seed(mission, &cohort, &entry.group))
    } else {
        let seed = group_seed(&read_master(root)?, mission, &cohort, &entry.group);
        (reference.variant(&seed).map_err(|e| ToolError(format!("variante du groupe {} : {e}", entry.group)))?, seed)
    };
    let text = plaintext(root, &entry.group, &variant, &seed, Some(hook))?;
    Ok(Some(GroupContent { current: entry.pack_id == pack_id, cohort, group: entry.group, text }))
}

/// The recorded groups of `cohort` for `mission` with the seed their pack content was drawn from (gen-packs: HMAC of
/// the master key; test-pack: the test seed), after the instructor code.
pub fn cohort_seeds(root: &Path, mission: &str, cohort: &str, codes: &mut dyn CodeSource) -> Result<Vec<(String, [u8; 32])>, ToolError> {
    check_name("mission", mission)?;
    check_name("cohorte", cohort)?;
    authenticate(root, codes)?;
    let master = read_master(root)?;
    let cohort = cohort_name(root, cohort);
    let entries = load_groups(&groups_path(root, &cohort, mission))?;
    if entries.is_empty() {
        return fail(format!("aucun groupe enregistré pour la cohorte {cohort}, mission {mission}"));
    }
    Ok(entries
        .into_iter()
        .map(|e| {
            let seed = if e.variant.as_deref() == Some("v0") { test_seed(mission, &cohort, &e.group) } else { group_seed(&master, mission, &cohort, &e.group) };
            (e.group, seed)
        })
        .collect())
}

/// `test-pack`: the reference variant (V0) sealed for an instructor test group, recorded with `"variant": "v0"`.
pub fn test_pack(
    root: &Path,
    mission: &str,
    cohort: &str,
    group: &str,
    replace: bool,
    codes: &mut dyn CodeSource,
    out: &mut dyn Write,
) -> Result<Generated, ToolError> {
    test_pack_with(root, mission, cohort, group, replace, codes, out, None)
}

/// [`test_pack`] whose sealed content comes from `hook`; the seed of the test pack is derived from the mission, the
/// cohort and the group (no master key: V0 is the instructor's reference).
#[allow(clippy::too_many_arguments)]
pub fn test_pack_with(
    root: &Path,
    mission: &str,
    cohort: &str,
    group: &str,
    replace: bool,
    codes: &mut dyn CodeSource,
    out: &mut dyn Write,
    hook: Option<ContentHook>,
) -> Result<Generated, ToolError> {
    check_name("mission", mission)?;
    check_name("cohorte", cohort)?;
    check_name("groupe", group)?;
    authenticate(root, codes)?;
    let reference = load_reference_pack(root, mission)?;
    let cohort = cohort_name(root, cohort);
    let cohort = cohort.as_str();
    let gpath = groups_path(root, cohort, mission);
    let mut entries = load_groups(&gpath)?;
    refuse_existing(&entries, &[group.to_string()], replace)?;
    let t = Target { root, mission, cohort };
    let name = group_name(&entries, group);
    let seed = test_seed(mission, cohort, &name);
    let content = plaintext(root, &name, &reference, &seed, hook)?;
    let g = write_group_pack(&t, &name, &content, Some("v0"), &mut entries)?;
    say(out, format!("Pack de test V0 (référence) : cohorte {cohort}, mission {mission}."));
    print_table(out, root, std::slice::from_ref(&g));
    say(out, format!("Codes enregistrés dans {} (dossier encadrant uniquement).", rel_display(root, &gpath)));
    Ok(g)
}

/// Seed of a V0 test pack's hidden tests: SHA-256 of a fixed label and the length-prefixed names.
fn test_seed(mission: &str, cohort: &str, group: &str) -> [u8; 32] {
    let mut data = b"n7rt-test-pack".to_vec();
    for part in [mission, cohort, group] {
        data.extend_from_slice(&(part.len() as u32).to_be_bytes());
        data.extend_from_slice(part.as_bytes());
    }
    let digest = sha256_hex(&data);
    let mut seed = [0u8; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&digest[2 * i..2 * i + 2], 16).unwrap_or(0);
    }
    seed
}

fn rel_display(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

// ---------------------------------------------------------------------------------------------------- judge

/// Result of `judge`. `detail` is private (instructor only).
#[derive(Debug)]
pub struct Judgement {
    pub pass: bool,
    pub detail: String,
    pub mission: String,
    pub cohort: String,
    pub group: String,
    pub pack_id: String,
    /// Start of the run, as the run file gives it (a checked UTC stamp).
    pub started_utc: String,
    /// The judgement record written: a new file, never one of an earlier judgement.
    pub judged_file: PathBuf,
}

/// The `integrity` field of a run file, read without building the rest of the document.
#[derive(Deserialize)]
struct RunIntegrity {
    integrity: String,
}

/// Finds the cohort and entry of a pack_id among `cohorts/*/<mission>/groups.json`.
fn find_pack(root: &Path, mission: &str, pack_id: &str) -> Result<Option<(String, GroupEntry)>, ToolError> {
    let cohorts = root.join("cohorts");
    let dirs = match fs::read_dir(&cohorts) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return fail(format!("lecture de {} : {e}", cohorts.display())),
    };
    let mut names: Vec<String> = dirs
        .filter_map(Result::ok)
        .filter(|d| d.path().is_dir())
        .filter_map(|d| d.file_name().into_string().ok())
        .collect();
    names.sort();
    for cohort in names {
        for e in load_groups(&groups_path(root, &cohort, mission))? {
            if e.pack_id == pack_id || e.previous_pack_ids.iter().any(|p| p == pack_id) {
                return Ok(Some((cohort, e)));
            }
        }
    }
    Ok(None)
}

fn safe_file_stem(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let s: String =
        stem.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    if s.trim_matches('.').is_empty() {
        "essai".into()
    } else {
        s
    }
}

/// `judge`: rebuilds the full pack of the run's group (V0 or the group variant), judges the run, prints the verdict and
/// the private detail, and writes the judgement as a new file `cohorts/<c>/<m>/judged/<run file stem>.json` (`-2`,
/// `-3`… before `.json` when the name is taken: an earlier judgement is never replaced). The run file is refused
/// over [`MAX_RUN_BYTES`] before it is read, and when one of its text fields is outside its form (n7rt-vehicle
/// run files): nothing from it reaches the console unchecked.
pub fn judge(root: &Path, run_path: &Path, codes: &mut dyn CodeSource, out: &mut dyn Write) -> Result<Judgement, ToolError> {
    if !n7rt_vehicle::ORACLE {
        return fail(ORACLE_ABSENT);
    }
    authenticate(root, codes)?;
    let bytes = read_bounded(run_path, MAX_RUN_BYTES, "fichier d'essai")?;
    let run = RunRecord::from_file_bytes(&bytes).map_err(|e| ToolError(format!("fichier d'essai invalide : {e}")))?;
    check_name("mission", &run.mission)?;
    let Some((cohort, entry)) = find_pack(root, &run.mission, &run.pack_id)? else {
        return fail(format!(
            "pack inconnu : aucun groupe de la mission {} n'a le pack_id {} dans ce dossier encadrant",
            run.mission, run.pack_id
        ));
    };
    if run.cohort != cohort || !run.group.eq_ignore_ascii_case(&entry.group) {
        return fail(format!(
            "le fichier d'essai annonce {}/{} mais son pack appartient à {cohort}/{}",
            run.cohort, run.group, entry.group
        ));
    }
    let reference = load_reference_pack(root, &run.mission)?;
    let pack = if entry.variant.as_deref() == Some("v0") {
        reference
    } else {
        let master = read_master(root)?;
        let seed = group_seed(&master, &run.mission, &cohort, &entry.group);
        reference.variant(&seed).map_err(|e| ToolError(format!("variante du groupe {} : {e}", entry.group)))?
    };
    let verdict = judge_run(&pack, &run).map_err(|e| ToolError(format!("essai non jugeable : {e}")))?;
    let label = if verdict.pass { "réussi" } else { "non réussi" };

    // The file name is the student's too: shown without its control characters.
    let run_name: String = run_path
        .file_name()
        .map(|n| n.to_string_lossy().chars().map(|c| if c.is_control() { '?' } else { c }).collect())
        .unwrap_or_default();
    let integrity = serde_json::from_slice::<RunIntegrity>(&bytes).ok().map(|r| r.integrity);
    let record = serde_json::json!({
        "format": "n7rt-judgement",
        "version": 1,
        "run_file": run_name,
        "mission": run.mission,
        "cohort": cohort,
        "group": entry.group,
        "pack_id": run.pack_id,
        "variant": entry.variant.clone().unwrap_or_else(|| "group".into()),
        "board_uid": run.board_uid,
        "started_utc": run.started_utc,
        "pass": verdict.pass,
        "verdict": label,
        "detail": verdict.detail,
        "run_integrity": integrity,
        "run_sha256": sha256_hex(&bytes),
        "judged_utc": utc_now(),
    });
    let mut text = serde_json::to_string_pretty(&record).expect("serializable");
    text.push('\n');
    let judged_dir = cohort_dir(root, &cohort, &run.mission).join("judged");
    let judged_file = write_new(&judged_dir, &safe_file_stem(run_path), "json", text.as_bytes())?;

    say(out, format!("Essai : {run_name} (cohorte {cohort}, mission {}, groupe {})", run.mission, entry.group));
    say(out, format!("VERDICT : {label}"));
    say(out, "Détail (privé, ne pas transmettre tel quel) :");
    say(out, &verdict.detail);
    say(out, format!("Jugement enregistré : {}", rel_display(root, &judged_file)));
    Ok(Judgement {
        pass: verdict.pass,
        detail: verdict.detail,
        mission: run.mission,
        cohort,
        group: entry.group,
        pack_id: run.pack_id,
        started_utc: run.started_utc,
        judged_file,
    })
}

// -------------------------------------------------------------------------------------------------- inspect

/// `inspect`: clear header of a sealed pack, or summary of a run file (integrity checked). Shows no secret. A pack over
/// [`MAX_PACK_BYTES`] or a run file over [`MAX_RUN_BYTES`] is refused before it is read.
pub fn inspect(file: &Path, out: &mut dyn Write) -> Result<(), ToolError> {
    let ext = file.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("n7rtpack") => inspect_pack(&read_bounded(file, MAX_PACK_BYTES, "pack")?, out),
        Some("n7rtrun") => inspect_run(&read_bounded(file, MAX_RUN_BYTES, "fichier d'essai")?, out),
        _ => {
            let bytes = read_bounded(file, MAX_RUN_BYTES, "fichier")?;
            let pack = bytes.len() as u64 <= MAX_PACK_BYTES && read_pack_header(&bytes).is_ok();
            if pack {
                inspect_pack(&bytes, out)
            } else {
                inspect_run(&bytes, out)
                    .map_err(|_| ToolError("fichier non reconnu (ni pack .n7rtpack, ni fichier d'essai .n7rtrun)".into()))
            }
        }
    }
}

fn inspect_pack(bytes: &[u8], out: &mut dyn Write) -> Result<(), ToolError> {
    let h = read_pack_header(bytes).map_err(|e| ToolError(e.to_string()))?;
    let v: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
    let kdf = &v["kdf"];
    say(out, "Pack de groupe chiffré (.n7rtpack)");
    say(out, format!("  Mission    : {}", h.mission));
    say(out, format!("  Cohorte    : {}", h.cohort));
    say(out, format!("  Groupe     : {}", h.group));
    say(out, format!("  pack_id    : {}", h.pack_id));
    say(out, format!("  Clé        : {}, {} Kio, {} passe(s), {} voie(s)", shown(&kdf["alg"]), kdf["m_kib"], kdf["t"], kdf["p"]));
    say(out, format!("  Chiffrement: {}", shown(&v["aead"]["alg"])));
    say(out, format!("  Taille     : {} octets", bytes.len()));
    say(out, format!("  SHA-256    : {}", sha256_hex(bytes)));
    Ok(())
}

fn inspect_run(bytes: &[u8], out: &mut dyn Write) -> Result<(), ToolError> {
    let r = RunRecord::from_file_bytes(bytes).map_err(|e| ToolError(format!("fichier d'essai invalide : {e}")))?;
    say(out, "Fichier d'essai (.n7rtrun), empreinte d'intégrité vérifiée");
    say(out, format!("  Mission     : {}", r.mission));
    say(out, format!("  Cohorte     : {}", r.cohort));
    say(out, format!("  Groupe      : {}", r.group));
    say(out, format!("  pack_id     : {}", r.pack_id));
    say(out, format!("  Application : {}", r.app_version));
    say(out, format!("  Carte       : {} (runtime {}, CRC du code étudiant {})", r.board_uid, r.runtime_build, r.student_crc32));
    say(out, format!("  Début (UTC) : {}", r.started_utc));
    say(out, format!("  Durée       : {:.3} s, tick {:.1} ms", r.duration_us as f64 / 1e6, r.tick_us as f64 / 1e3));
    say(out, format!("  Trames      : {}", r.frames.len()));
    say(out, format!("  Ticks carte : {}", r.ticks.len()));
    say(out, format!("  SHA-256     : {}", sha256_hex(bytes)));
    Ok(())
}

// ------------------------------------------------------------------------------------------------------ time

fn utc_now() -> String {
    utc_string(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}

/// `YYYY-MM-DDTHH:MM:SSZ` of a Unix time (civil-from-days, proleptic Gregorian).
pub fn utc_string(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn utc_vectors() {
        assert_eq!(utc_string(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_string(1_791_290_096), "2026-10-06T12:34:56Z");
        assert_eq!(utc_string(1_709_251_199), "2024-02-29T23:59:59Z");
        assert_eq!(utc_string(951_868_800), "2000-03-01T00:00:00Z");
    }

    #[test]
    fn names() {
        for ok in ["apps", "G01", "2026-27", "essais", "groupe_3"] {
            assert!(check_name("groupe", ok).is_ok(), "{ok}");
        }
        for bad in ["", "-x", "a/b", "..", "a b", "é", "x\\y", &"a".repeat(33)] {
            assert!(check_name("groupe", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hex_key() {
        let k: [u8; 32] = core::array::from_fn(|i| i as u8 * 7);
        assert_eq!(parse_hex32(&hex(&k)), Some(k));
        assert_eq!(parse_hex32(&hex(&k).to_uppercase()), Some(k));
        assert_eq!(parse_hex32("00"), None);
        assert_eq!(parse_hex32(&"zz".repeat(32)), None);
    }

    #[test]
    fn parse_commands() {
        let c = parse_args(&args("gen-packs D --mission apps --cohort 2026 --groups G01,G02,,G03 --replace")).unwrap();
        assert_eq!(
            c.command,
            Command::GenPacks {
                dir: "D".into(),
                mission: "apps".into(),
                cohort: "2026".into(),
                groups: vec!["G01".into(), "G02".into(), "G03".into()],
                replace: true
            }
        );
        assert!(!c.code_stdin);
        let c = parse_args(&args("test-pack D --mission=apps --group TEST --code-stdin")).unwrap();
        assert_eq!(
            c.command,
            Command::TestPack { dir: "D".into(), mission: "apps".into(), cohort: "essais".into(), group: "TEST".into(), replace: false }
        );
        assert!(c.code_stdin);
        assert_eq!(parse_args(&args("judge D r.n7rtrun")).unwrap().command, Command::Judge { dir: "D".into(), run: "r.n7rtrun".into() });
        assert_eq!(parse_args(&args("inspect f")).unwrap().command, Command::Inspect { file: "f".into() });
        assert_eq!(parse_args(&args("set-code D")).unwrap().command, Command::SetCode { dir: "D".into() });
        assert_eq!(parse_args(&args("verify-code D --code-stdin")).unwrap().command, Command::VerifyCode { dir: "D".into() });
        assert_eq!(parse_args(&args("--help")).unwrap().command, Command::Help);
        assert_eq!(parse_args(&args("set-code --help")).unwrap().command, Command::Help);
    }

    #[test]
    fn parse_refusals_never_echo_values() {
        for (line, frag) in [
            ("verify-code D --code SECRETCODE123", "jamais"),
            ("verify-code D --code=SECRETCODE123", "jamais"),
            ("set-code D --password=SECRETCODE123", "jamais"),
            ("verify-code D SECRETCODE123", "en trop"),
            ("verify-code D -c=SECRETCODE123", "option inconnue"),
            ("gen-packs D --mission apps --groups G01", "--cohort obligatoire"),
            ("gen-packs D --mission apps --cohort c --groups ,", "vide"),
            ("gen-packs D --mission --cohort c", "valeur manquante"),
            ("set-code D --replace", "non prise en charge"),
            ("inspect f --code-stdin", "non prise en charge"),
            ("judge D", "fichier d'essai"),
            ("set-code", "dossier encadrant"),
            ("frobnicate D", "commande inconnue"),
            ("verify-code D --replace=SECRETCODE123", "valeur"),
            ("gen-packs D --mission a --mission b", "répétée"),
        ] {
            let e = parse_args(&args(line)).unwrap_err().0;
            assert!(e.contains(frag), "{line} → {e}");
            assert!(!e.contains("SECRETCODE123"), "{line} → {e}");
        }
        assert!(parse_args(&[]).is_err());
    }
}
