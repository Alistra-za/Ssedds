use clap::{Parser, Subcommand};
use sdc_bench::{corpus::bench_root, gates};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "sdc-bench",
    about = "sdc regression harness over F:/ai/dds/bench (SDC_BENCH_ROOT)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a named gate (s3, ...) and print its report.
    Gate { name: String },
    /// Record SHA-256s of u5 flag-off BC7 payloads of every bench image (reference for the patch bit-identity gate).
    DumpBc7e {
        #[arg(long)]
        out: std::path::PathBuf,
    },
}

fn main() -> ExitCode {
    let root = bench_root();
    match Cli::parse().cmd {
        Cmd::Gate { name } => match gates::run(&name, &root) {
            Ok(reports) => {
                let mut ok = true;
                for r in &reports {
                    println!("{r}");
                    ok &= r.pass;
                }
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::from(3)
            }
        },
        Cmd::DumpBc7e { out } => match gates::dump_bc7e(&root, &out) {
            Ok(n) => {
                println!("{n}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::from(3)
            }
        },
    }
}
