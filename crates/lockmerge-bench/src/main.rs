fn main() {
    std::process::exit(lockmerge_bench::cli::main(
        std::env::args().skip(1).collect(),
    ));
}
