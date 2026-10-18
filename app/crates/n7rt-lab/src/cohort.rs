//! Instructor tools of the application (docs/distribution.md §3.3, §4): the group packs of a cohort and the judgement
//! of a student's run file, through the library of the console tool (`n7rt_tool::gen_packs`, `n7rt_tool::judge`):
//! same files, same `groups.json`, same judgement as `n7rt-tool`. Reached in instructor mode only; the instructor code
//! verified at login is handed to the tool, which checks it again against `instructor/auth.json`. The tool's console
//! output (group codes, private detail) goes nowhere: what the instructor sees comes back as values.
//!
//! Errors are the tool's French sentences, with the instructor directory written « dossier encadrant ».

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use n7rt_tool::{CodeSource, ToolError};
use serde::Serialize;

use crate::access::{MAX_RUN_BYTES, MSG_RUN_TOO_LARGE, RUN_EXT};
use crate::mission::guarded;

/// A generated group pack, for the instructor's table.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GenRow {
    pub group: String,
    pub code: String,
    /// Relative to the instructor directory, `/`-separated.
    pub file: String,
}

/// Verdict of a student's run file (instructor only: the detail is private).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Judgement {
    pub mission: String,
    pub cohort: String,
    pub group: String,
    pub started_utc: String,
    /// "pass" | "fail"
    pub verdict: &'static str,
    pub detail: String,
    /// Where the tool kept the judgement (a new file each time), relative to the instructor directory.
    pub judged_file: Option<String>,
}

/// The code verified at login, given to the tool for each command.
struct Held<'a>(&'a str);

impl CodeSource for Held<'_> {
    fn read_code(&mut self, _prompt: &str) -> Result<String, ToolError> {
        Ok(self.0.to_owned())
    }
}

fn relative(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// The tool's message for the UI: no absolute path of the instructor directory, a capital, a final period, the
/// command-line option named as the dialog names it.
fn message(root: &Path, e: &ToolError) -> String {
    let mut m = e.0.clone();
    for r in [root.display().to_string(), root.to_string_lossy().replace('\\', "/")] {
        if !r.is_empty() {
            m = m.replace(&r, "dossier encadrant");
        }
    }
    m = m.replace("--replace", "« Régénérer les groupes déjà présents »");
    let mut c = m.chars();
    let mut out: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

fn internal() -> String {
    "Erreur interne des outils encadrant.".into()
}

/// One sealed pack per group (the group's variant, a new code, a new pack id), recorded in the cohort's
/// `groups.json`; a group already there is refused unless `replace`.
pub fn gen_packs(root: &Path, code: &str, mission: &str, cohort: &str, groups: &[String], replace: bool) -> Result<Vec<GenRow>, String> {
    // A mission with a truth file gets release 0.3 packs (sealed workflow); otherwise the 0.2 content.
    let hook: n7rt_tool::ContentHook = &crate::workflow::compile::group_content;
    let done = guarded(|| n7rt_tool::gen_packs_with(root, mission, cohort, groups, replace, &mut Held(code), &mut io::sink(), Some(hook)));
    match done {
        Ok(Ok(rows)) => Ok(rows.into_iter().map(|g| GenRow { file: relative(root, &g.file), group: g.group, code: g.code }).collect()),
        Ok(Err(e)) => Err(message(root, &e)),
        Err(()) => Err(internal()),
    }
}

/// `essai-apps-G01-20261006-142233.n7rtrun` as given by the UI, reduced to a safe file name.
fn run_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut s: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    s = s.trim_matches('.').to_owned();
    if s.is_empty() {
        s = "essai".into();
    }
    if !s.ends_with(&format!(".{RUN_EXT}")) {
        s.push_str(&format!(".{RUN_EXT}"));
    }
    s
}

/// Judges a run file given by the UI (its name and bytes): the tool reads it from a private temporary folder, judges
/// it against the full pack of its group and keeps the judgement in the cohort's `judged/`, under the run file's name
/// (`-2`, `-3`… when it is taken: an earlier judgement is never replaced). The fields shown come from the tool, which
/// checked them.
pub fn judge(root: &Path, code: &str, name: &str, bytes: &[u8]) -> Result<Judgement, String> {
    if !n7rt_vehicle::ORACLE {
        return Err(crate::mission::ORACLE_ABSENT.into());
    }
    if bytes.len() > MAX_RUN_BYTES {
        return Err(MSG_RUN_TOO_LARGE.into());
    }
    static N: AtomicU64 = AtomicU64::new(0);
    let dir: PathBuf = std::env::temp_dir().join(format!("n7rt-judge-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let path = dir.join(run_name(name));
    let written = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, bytes));
    if written.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err("Fichier d'essai non lisible par l'outil encadrant (dossier temporaire inaccessible).".into());
    }
    let done = guarded(|| n7rt_tool::judge(root, &path, &mut Held(code), &mut io::sink()));
    let _ = std::fs::remove_dir_all(&dir);
    let j = match done {
        Ok(Ok(j)) => j,
        Ok(Err(e)) => return Err(message(root, &e)),
        Err(()) => return Err(internal()),
    };
    Ok(Judgement {
        mission: j.mission,
        cohort: j.cohort,
        group: j.group,
        started_utc: j.started_utc,
        verdict: if j.pass { "pass" } else { "fail" },
        detail: j.detail,
        judged_file: Some(relative(root, &j.judged_file)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_names_are_reduced_to_a_file_name() {
        assert_eq!(run_name("essai-apps-G01-20261006-142233.n7rtrun"), "essai-apps-G01-20261006-142233.n7rtrun");
        assert_eq!(run_name("C:\\fakepath\\..\\x y.n7rtrun"), "x_y.n7rtrun");
        assert_eq!(run_name("../../etc"), "etc.n7rtrun");
        assert_eq!(run_name(""), "essai.n7rtrun");
    }

    #[test]
    fn tool_messages_hide_the_instructor_directory() {
        let root = Path::new("C:/Users/x/n7rt-canlab-missions");
        let e = ToolError("groupe(s) déjà généré(s) : G01 (--replace pour les régénérer avec un nouveau code)".into());
        assert_eq!(message(root, &e), "Groupe(s) déjà généré(s) : G01 (« Régénérer les groupes déjà présents » pour les régénérer avec un nouveau code).");
        let e = ToolError(format!("lecture de {}/cohorts : refusé", root.display()));
        assert_eq!(message(root, &e), "Lecture de dossier encadrant/cohorts : refusé.");
        assert_eq!(relative(root, &root.join("cohorts").join("c1").join("x.json")), "cohorts/c1/x.json");
    }
}
