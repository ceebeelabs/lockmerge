//! The `lockmerge` binary. Everything of substance is in the library so that
//! tests can run it with a fake format.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(lockmerge_cli::main_with(&args));
}
