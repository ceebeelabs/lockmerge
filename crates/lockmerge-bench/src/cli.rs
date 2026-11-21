//! The command line of `lockmerge-bench`.
//!
//! ```text
//! lockmerge-bench check   [--fixtures DIR] [--release v1] [--driver-bin PATH] [--timeout-secs N]
//! lockmerge-bench survey  --sources FILE [--cache DIR]
//! lockmerge-bench extract --sources FILE --out DIR [--cache DIR] [--date YYYY-MM-DD]
//! ```
//!
//! Exit codes: 0 success, 1 the gate failed, 2 a usage or I/O error.
//! `survey` and `extract` need the network and run by hand on the owner side.

use crate::check::{CheckOpts, check};
use crate::extract::{ExtractOpts, extract};
use crate::oracle::{CargoOracle, Oracle};
use crate::registry::formats;
use crate::report::render;
use crate::scan::{Source, ensure_clone, parse_sources, survey};
use crate::util::today;
use std::path::PathBuf;
use std::time::Duration;

const USAGE: &str = "usage:
  lockmerge-bench check   [--fixtures DIR] [--release v1] [--driver-bin PATH] [--timeout-secs N]
  lockmerge-bench survey  --sources FILE [--cache DIR]
  lockmerge-bench extract --sources FILE --out DIR [--cache DIR] [--date YYYY-MM-DD]";

struct Args {
    rest: Vec<String>,
}

impl Args {
    /// Removes `--name value` and returns the value.
    fn take(&mut self, name: &str) -> Result<Option<String>, String> {
        match self.rest.iter().position(|a| a == name) {
            None => Ok(None),
            Some(i) => {
                if i + 1 >= self.rest.len() {
                    return Err(format!("{name} needs a value"));
                }
                let v = self.rest.remove(i + 1);
                self.rest.remove(i);
                Ok(Some(v))
            }
        }
    }

    fn finish(self) -> Result<(), String> {
        match self.rest.first() {
            None => Ok(()),
            Some(a) => Err(format!("unexpected argument `{a}`")),
        }
    }
}

fn load_sources(path: &str) -> Result<Vec<Source>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    parse_sources(&text)
}

fn oracle_for(src: &Source) -> Result<Box<dyn Oracle>, String> {
    match src.ecosystem.as_str() {
        "cargo" => Ok(Box::new(CargoOracle)),
        other => Err(format!("{}: no oracle for `{other}` yet", src.name)),
    }
}

fn run(mut argv: Vec<String>) -> Result<i32, String> {
    if argv.is_empty() {
        return Err("no command".into());
    }
    let command = argv.remove(0);
    let mut args = Args { rest: argv };
    match command.as_str() {
        "check" => {
            let fixtures = args
                .take("--fixtures")?
                .unwrap_or_else(|| "tests/fixtures".into());
            let mut opts = CheckOpts::new(PathBuf::from(fixtures));
            opts.release = args.take("--release")?;
            opts.driver_bin = args.take("--driver-bin")?.map(PathBuf::from);
            if let Some(s) = args.take("--timeout-secs")? {
                let n: u64 = s
                    .parse()
                    .map_err(|_| "--timeout-secs needs a number".to_string())?;
                opts.timeout = Duration::from_secs(n);
            }
            args.finish()?;
            let formats = formats();
            let outcome = check(&opts, &formats);
            print!("{}", render(&outcome, &formats));
            Ok(if outcome.passed() { 0 } else { 1 })
        }
        "survey" => {
            let sources = args.take("--sources")?.ok_or("--sources is required")?;
            let cache = args
                .take("--cache")?
                .unwrap_or_else(|| "bench-cache".into());
            args.finish()?;
            for src in load_sources(&sources)? {
                let repo = PathBuf::from(&cache).join(format!("{}.git", src.name));
                ensure_clone(&src, &repo)?;
                println!("{}", survey(&repo, &src, oracle_for(&src)?.as_ref())?);
            }
            Ok(0)
        }
        "extract" => {
            let sources = args.take("--sources")?.ok_or("--sources is required")?;
            let out = args.take("--out")?.ok_or("--out is required")?;
            let cache = args
                .take("--cache")?
                .unwrap_or_else(|| "bench-cache".into());
            let date = args.take("--date")?.unwrap_or_else(today);
            args.finish()?;
            let opts = ExtractOpts {
                out: PathBuf::from(out),
                cache: PathBuf::from(cache),
                date,
            };
            for src in load_sources(&sources)? {
                println!("{}", extract(&src, &opts)?);
            }
            Ok(0)
        }
        other => Err(format!("unknown command `{other}`")),
    }
}

pub fn main(argv: Vec<String>) -> i32 {
    match run(argv) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("lockmerge-bench: {e}\n{USAGE}");
            2
        }
    }
}
