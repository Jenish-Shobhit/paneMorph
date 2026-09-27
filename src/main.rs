//! The `panemorph` binary. herdr runs it through `./bin/panemorph` for every
//! plugin action and popup; see [`panemorph::actions`] for the commands.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(panemorph::actions::main_with(&args));
}
