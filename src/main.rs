use std::process::ExitCode;

use anyhow::Context as _;
use clap::Parser as _;

fn main() -> ExitCode {
    if let Err(err) = try_main() {
        eprintln!("{err:#}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn try_main() -> anyhow::Result<()> {
    template::logging::init().context("init logging")?;

    let cli = template::cli::Cli::parse();
    tracing::debug!(?cli, "parsed cli");

    match cli.command {
        template::cli::Command::Hello(args) => {
            let message = template::greeting::hello(args.name.as_deref());
            println!("{message}");
        }
    }

    Ok(())
}
