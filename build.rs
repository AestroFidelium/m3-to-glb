use clap::CommandFactory;
use clap_complete::{generate_to, shells::Zsh};
use std::env;
use std::fs;

// Pull in cli.rs so the Cli struct is available here.
include!("src/cli.rs");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_name = env::var("CARGO_PKG_NAME")?;
    let out_dir = "completions";

    fs::create_dir_all(out_dir)?;

    let mut cmd = Cli::command();

    // Generate the zsh completion file (`_m3-to-glb`) in `completions/`.
    generate_to(Zsh, &mut cmd, &app_name, out_dir)?;

    println!("cargo:rerun-if-changed=src/cli.rs");
    Ok(())
}
