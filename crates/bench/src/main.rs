use clap::{Parser, Subcommand};
use sdc_bench::{corpus::bench_root, gates};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "sdc-bench",
    about = "sdc regression harness over the bench corpus (SDC_BENCH_ROOT)"
)]
struct Cli {
    /// Run every gate (same as `gate all`).
    #[arg(long)]
    gates: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a named gate (s3, ..., or `all`) and print its report.
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
    /// Spike S7b: NVTT on the engine's DDS levels; BC6H per level down to 1x1.
    S7bNvtt,
    /// Write the S2 small-texture and S8 `_m` variant source PNGs (owner in-game spikes).
    GenSpikeAssets {
        #[arg(long)]
        out: std::path::PathBuf,
    },
    /// M6 Track D spec §7: BC4/BC5 search radius against PSNR and encode time (images2 mask_real, normal_real;
    /// read-only). Writes <out>/radius45.csv.
    Radius45 {
        /// A scratch folder outside the bench root.
        #[arg(long)]
        out: std::path::PathBuf,
    },
    /// BC7 tuning study (docs/research/bc7e-tuning.md): bc7e variants and the reference encoder over
    /// `<corpus>/<class>/*.png`, appending one CSV row per image and variant to --out.
    Bc7Tune {
        /// Corpus root with `diffuse`, `normal`, `cutout` and `smooth` folders of PNGs.
        #[arg(long)]
        corpus: std::path::PathBuf,
        /// CSV to append to (created with a header when missing).
        #[arg(long)]
        out: std::path::PathBuf,
        /// Variant names (default: all); `--variants none` runs only the reference.
        #[arg(long, value_delimiter = ',')]
        variants: Vec<String>,
        /// At most this many images per class.
        #[arg(long)]
        images: Option<usize>,
        /// Run the reference encoder with this many fully fitted partitions per mode (0 = off).
        #[arg(long, default_value_t = 0)]
        reference: usize,
        /// Timing repeats per variant (the fastest is kept).
        #[arg(long, default_value_t = 1)]
        repeats: usize,
    },
}

/// S2: 200 varied 16-128 px sources; S8: constant-alpha `_m` variants plus a height-mask gradient.
/// Returns the number of files written.
fn gen_spike_assets(out: &std::path::Path) -> anyhow::Result<usize> {
    let sizes = [16u32, 32, 64, 128];
    std::fs::create_dir_all(out.join("s2"))?;
    std::fs::create_dir_all(out.join("s8"))?;
    let mut count = 0;
    for n in 0..200u32 {
        let s = sizes[(n % 4) as usize];
        let img = image::ImageBuffer::from_fn(s, s, |x, y| {
            image::Rgba([
                (x * 255 / s) as u8,
                (y * 255 / s) as u8,
                (n * 37 % 256) as u8,
                255,
            ])
        });
        img.save(out.join(format!("s2/small_{n:03}_{s}.png")))?;
        count += 1;
    }
    for a in [245u8, 248, 250, 252, 255] {
        image::ImageBuffer::from_fn(64, 64, |_, _| image::Rgba([80u8, 160, 40, a]))
            .save(out.join(format!("s8/m_{a}_m.png")))?;
        count += 1;
    }
    image::ImageBuffer::from_fn(64, 64, |x, _| image::Rgba([0u8, 0, 0, (x * 4) as u8]))
        .save(out.join("s8/heightmask_m.png"))?;
    Ok(count + 1)
}

/// Print every report on stdout; exit 0 when all pass, 1 when any fails, 3 on an error before a verdict.
fn print_reports(r: anyhow::Result<Vec<gates::GateReport>>) -> ExitCode {
    match r {
        Ok(reports) => {
            let ok = reports.iter().all(|r| r.pass);
            for r in &reports {
                println!("{r}");
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
    }
}

fn main() -> ExitCode {
    let root = bench_root();
    let cli = Cli::parse();
    if cli.gates {
        return print_reports(gates::run("all", &root));
    }
    let Some(cmd) = cli.cmd else {
        eprintln!("use --gates or a subcommand");
        return ExitCode::from(2);
    };
    match cmd {
        Cmd::Gate { name } => print_reports(gates::run(&name, &root)),
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
        Cmd::S7Nvtt | Cmd::S7bNvtt => {
            let b = matches!(cmd, Cmd::S7bNvtt);
            let work = std::path::PathBuf::from(if b {
                "target/bench/s7b"
            } else {
                "target/bench/s7"
            });
            std::fs::create_dir_all(&work).unwrap();
            let r = if b {
                sdc_bench::s7b::run(&root, &work)
            } else {
                sdc_bench::s7::run(&root, &work)
            };
            match r {
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
        Cmd::GenSpikeAssets { out } => match gen_spike_assets(&out) {
            Ok(n) => {
                println!("wrote {n} files to {}", out.display());
                ExitCode::SUCCESS
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
        Cmd::Bc7Tune {
            corpus,
            out,
            variants,
            images,
            reference,
            repeats,
        } => {
            let variants: Vec<String> = variants.into_iter().filter(|v| v != "none").collect();
            match sdc_bench::bc7tune::run(&corpus, &out, &variants, images, reference, repeats) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("bc7tune: {e:#}");
                    ExitCode::from(3)
                }
            }
        }
        Cmd::Radius45 { out } => match sdc_bench::radius::measure(&root, &out) {
            Ok(rows) => {
                let s = sdc_bench::radius::summary(&rows);
                println!("format radius mean_psnr_db total_secs");
                for (f, r, p, t) in &s {
                    println!("{f} {r} {p:.4} {t:.4}");
                }
                let (q, fast) = sdc_bench::radius::decide(&s);
                println!("rule Q (quality default -> max): {q}; rule F (fast radius): {fast:?}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("radius45: {e:#}");
                ExitCode::FAILURE
            }
        },
    }
}
