//! `n7rt-tool` console entry point; the commands live in the library (docs/distribution.md §4).

use n7rt_tool::{parse_args, run, LineCodes, TtyCodes, EXIT_ERROR, USAGE};

fn main() {
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let code = match parse_args(&args) {
        Err(e) => {
            eprintln!("erreur : {e}");
            if args.is_empty() {
                eprintln!("\n{}", USAGE.trim_end());
            }
            EXIT_ERROR
        }
        Ok(cli) => {
            let mut out = std::io::stdout().lock();
            let result = if cli.code_stdin {
                run(&cli, &mut LineCodes(std::io::stdin().lock()), &mut out)
            } else {
                run(&cli, &mut TtyCodes, &mut out)
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
