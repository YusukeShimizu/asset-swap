use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Hello(HelloArgs),
}

#[derive(Debug, Args)]
pub struct HelloArgs {
    #[arg(long)]
    pub name: Option<String>,
}
