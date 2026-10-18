//! `n7rt-pack`: the commands of `n7rt-tool` whose generated packs carry the release 0.3 sealed workflow when the mission
//! has a truth file (`missions/<m>/workflow-v0.json` of the instructor directory). Instructor edition only: the public
//! edition cannot render the expected outputs and refuses such a mission.
//!
//! Two commands of its own: `verify-export <dossier encadrant> <dossier d'export>` checks a group's final fiche against
//! the sealed content of the group its pack belongs to (regenerated from the master key); `variantes` lists the
//! incident variant each group of a cohort drew.

use std::path::Path;

use n7rt_tool::{cohort_seeds, group_content_of, parse_args, run_with, CodeSource, ContentHook, LineCodes, TtyCodes, EXIT_ERROR, USAGE};

const VERIFY_USAGE: &str = "  n7rt-pack verify-export <dossier encadrant> <dossier d'export> [--code-stdin]
      Vérifie la fiche finale exportée d'un groupe (dossier <mission>-<groupe>-<date> de ses Documents) : copie
      scellée intacte avec la clé du pack du groupe, pack actuel du groupe, mission clôturée, tentatives liées
      (nonce, session, empreintes du banc et du bus), fiche et diagnostics conformes à la variante du groupe.
      Code de sortie 0 si l'export est conforme, 1 sinon.
  n7rt-pack variantes <dossier encadrant> --mission <mission> --cohort <cohorte> [--code-stdin]
      Liste, pour chaque groupe de la cohorte, la variante de chacun de ses incidents (ligne de catalogue
      et paramètre), pour l'accompagner avec les cartes du manuel encadrant. Dossier encadrant uniquement.";

/// Exit code of a refused export (the export was read and judged).
const EXIT_REFUSED: i32 = 1;

fn verify_export(args: &[String], hook: ContentHook) -> i32 {
    let code_stdin = args.iter().any(|a| a == "--code-stdin");
    let positional: Vec<&String> = args.iter().skip(1).filter(|a| a.as_str() != "--code-stdin").collect();
    if positional.len() != 2 || positional.iter().any(|a| a.starts_with("--")) {
        eprintln!("erreur : verify-export : dossier encadrant et dossier d'export attendus\n\n{VERIFY_USAGE}");
        return EXIT_ERROR;
    }
    let (root, folder) = (Path::new(positional[0].as_str()), Path::new(positional[1].as_str()));
    let header = std::fs::read(folder.join("fiche.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .map(|d| (d["mission"].as_str().unwrap_or("").to_owned(), d["pack_id"].as_str().unwrap_or("").to_owned()));
    let Some((mission, pack_id)) = header else {
        println!("EXPORT REFUSÉ : fiche.json absent ou illisible dans {}", folder.display());
        return EXIT_REFUSED;
    };
    let mut stdin_codes;
    let mut tty = TtyCodes;
    let codes: &mut dyn CodeSource = if code_stdin {
        stdin_codes = LineCodes(std::io::stdin().lock());
        &mut stdin_codes
    } else {
        &mut tty
    };
    match group_content_of(root, &mission, &pack_id, codes, hook) {
        Err(e) => {
            eprintln!("erreur : {e}");
            EXIT_ERROR
        }
        Ok(None) => {
            println!("EXPORT REFUSÉ : aucun groupe de la mission {mission} n'a ce pack dans ce dossier encadrant (autre cohorte ou autre dossier)");
            EXIT_REFUSED
        }
        Ok(Some(g)) if !g.current => {
            println!("EXPORT REFUSÉ : pack remplacé depuis (--replace) : export ancien du groupe {}", g.group);
            EXIT_REFUSED
        }
        Ok(Some(g)) => match n7rt_lab::workflow::export::verify(folder, &mission, &g.cohort, &g.group, &g.text) {
            Ok(lines) => {
                println!("EXPORT CONFORME");
                for l in lines {
                    println!("  {l}");
                }
                0
            }
            Err(why) => {
                println!("EXPORT REFUSÉ : {why}");
                EXIT_REFUSED
            }
        },
    }
}

/// `variantes`: the incident variant of each group of a cohort (instructor directory only: it reads the truth file).
fn variants(args: &[String]) -> i32 {
    let mut values = std::collections::BTreeMap::new();
    let (mut positional, mut code_stdin, mut it) = (Vec::new(), false, args.iter().skip(1));
    while let Some(a) = it.next() {
        match a.as_str() {
            "--code-stdin" => code_stdin = true,
            "--mission" | "--cohort" => {
                let Some(v) = it.next() else {
                    eprintln!("erreur : variantes : valeur manquante après {a}\n\n{VERIFY_USAGE}");
                    return EXIT_ERROR;
                };
                values.insert(a.trim_start_matches('-').to_owned(), v.clone());
            }
            _ if a.starts_with("--") => {
                eprintln!("erreur : variantes : option inconnue {a}\n\n{VERIFY_USAGE}");
                return EXIT_ERROR;
            }
            _ => positional.push(a.clone()),
        }
    }
    let (Some(mission), Some(cohort), [root]) = (values.get("mission"), values.get("cohort"), positional.as_slice()) else {
        eprintln!("erreur : variantes : dossier encadrant, --mission et --cohort attendus\n\n{VERIFY_USAGE}");
        return EXIT_ERROR;
    };
    let root = Path::new(root.as_str());
    let truth: serde_json::Value = match std::fs::read(root.join("missions").join(mission).join("workflow-v0.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        Some(t) => t,
        None => {
            eprintln!("erreur : missions/{mission}/workflow-v0.json absent ou illisible dans le dossier encadrant");
            return EXIT_ERROR;
        }
    };
    let mut stdin_codes;
    let mut tty = TtyCodes;
    let codes: &mut dyn CodeSource = if code_stdin {
        stdin_codes = LineCodes(std::io::stdin().lock());
        &mut stdin_codes
    } else {
        &mut tty
    };
    let seeds = match cohort_seeds(root, mission, cohort, codes) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("erreur : {e}");
            return EXIT_ERROR;
        }
    };
    let incidents = truth["incidents"].as_array().cloned().unwrap_or_default();
    use n7rt_lab::workflow::compile::{group_incident, incident_variant, incident_variants};
    println!("Cohorte {cohort}, mission {mission} : variante de chaque incident par groupe (0 = carte du manuel)");
    for (group, seed) in &seeds {
        println!("{group}");
        for (i, inc) in incidents.iter().enumerate() {
            let drawn = match group_incident(inc, seed, i + 1) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("erreur : incident {} : {e}", i + 1);
                    return EXIT_ERROR;
                }
            };
            let n = incident_variants(inc).unwrap_or(0);
            let k = (0..=n).find(|k| incident_variant(inc, *k).ok().as_ref() == Some(&drawn)).unwrap_or(0);
            println!("  {} variante {k}/{n} : {}", drawn["label"].as_str().unwrap_or("?"), drawn["line"].as_str().unwrap_or("?"));
        }
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let hook: ContentHook = &n7rt_lab::workflow::compile::group_content;
    if args.first().map(String::as_str) == Some("verify-export") {
        std::process::exit(verify_export(&args, hook));
    }
    if args.first().map(String::as_str) == Some("variantes") {
        std::process::exit(variants(&args));
    }
    let code = match parse_args(&args) {
        Err(e) => {
            eprintln!("erreur : {e}");
            if args.is_empty() {
                eprintln!("\n{}\n{VERIFY_USAGE}", USAGE.trim_end());
            }
            EXIT_ERROR
        }
        Ok(cli) => {
            let mut out = std::io::stdout().lock();
            let result = if cli.code_stdin {
                run_with(&cli, &mut LineCodes(std::io::stdin().lock()), &mut out, Some(hook))
            } else {
                run_with(&cli, &mut TtyCodes, &mut out, Some(hook))
            };
            match result {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("erreur : {e}");
                    EXIT_ERROR
                }
            }
        }
    };
    std::process::exit(code);
}
