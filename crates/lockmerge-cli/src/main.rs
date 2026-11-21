//! The `lockmerge` binary. Everything of substance is in the library so that
//! tests can run it with a fake format.

fn main() {
    // `std::env::args()` panics on a non-UTF-8 argument, outside the driver's
    // `catch_unwind` (exit 101). Convert explicitly instead: exit 255 without
    // having touched any file (ADR 0003: internal errors leave `%A` alone).
    let mut args = Vec::new();
    for a in std::env::args_os().skip(1) {
        match a.into_string() {
            Ok(s) => args.push(s),
            Err(raw) => {
                eprintln!("lockmerge: internal error: argument {raw:?} is not valid UTF-8");
                std::process::exit(lockmerge_cli::driver::EXIT_ERROR);
            }
        }
    }
    std::process::exit(lockmerge_cli::main_with(&args));
}
