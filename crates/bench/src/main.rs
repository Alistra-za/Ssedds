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
    /// Throughput of ISPC bc7e u5 over rgb_real + alpha_real (for spike S4).
    SpeedBc7e,
    /// Spike S7: NVTT fed one mip at a time.
    S7Nvtt,
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
        Cmd::SpeedBc7e => {
            encoders_sys::init(encoders_sys::Bc1Model::Ideal).unwrap();
            let mut imgs = vec![];
            for set in ["rgb_real", "alpha_real"] {
                for f in sdc_bench::corpus::set_files(&root, "images", set, "png").unwrap() {
                    imgs.push(sdc_bench::corpus::load_png_rgba8(&f).unwrap());
                }
            }
            let mpix: f64 = imgs.iter().map(|i| (i.width * i.height) as f64 / 1e6).sum();
            let run = || {
                let t = std::time::Instant::now();
                for img in &imgs {
                    let _ = sdc_bench::codec::encode_bc7_image(img, gates::U5);
                }
                mpix / t.elapsed().as_secs_f64()
            };
            let all = run();
            let one = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .unwrap()
                .install(run);
            println!(
                "ISPC u5: {one:.3} Mpix/s single-thread, {all:.3} Mpix/s on {} threads",
                rayon::current_num_threads()
            );
            ExitCode::SUCCESS
        }
        Cmd::S7Nvtt => {
            let work = std::path::PathBuf::from("target/bench/s7");
            std::fs::create_dir_all(&work).unwrap();
            match sdc_bench::s7::run(&root, &work) {
                Ok(r) => {
                    println!("{r}");
                    if r.pass {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(1)
                    }
                }
                Err(e) => {
                    eprintln!("error: {e:#}");
                    ExitCode::from(3)
                }
            }
        }
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
