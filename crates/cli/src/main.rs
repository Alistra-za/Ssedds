//! `sdc` command line (spec §8). Human output goes to stderr; with `--json`, stdout carries only JSON Lines.
//! Exit codes, highest precedence first: 3 fatal > 2 usage/config > 1 partial > 4 validation failed > 0.
/// `eprintln!` that ignores write errors: a closed stderr is not a crash, and a panic while reporting one would end
/// the process with 101 outside `catch_unwind` (trace-debug #18, Note BQ).
macro_rules! err {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr().lock(), $($t)*);
    }};
}

mod args;
mod cancel;
mod human;
mod verbs;

use args::*;
use clap::Parser;
use engine::also::{run_also, AlsoSpec};
use engine::api::{CancelToken, Engine, EngineError};
use engine::events::{new_batch_id, EventSink, JsonLinesSink, Reporter};
use engine::session::AnalysisCache;
use std::collections::BTreeSet;
use std::process::ExitCode;
use std::sync::Arc;

/// Human output on stderr, plus JSON Lines on stdout with `--json`. `run_also` takes this and makes its own reporters.
fn tee_sink(json: bool) -> Arc<dyn EventSink> {
    let mut sinks: Vec<Arc<dyn EventSink>> = vec![Arc::new(human::HumanSink::new())];
    if json {
        sinks.push(Arc::new(JsonLinesSink::new(std::io::stdout())));
    }
    Arc::new(human::TeeSink(sinks))
}

fn reporter(json: bool) -> Reporter {
    Reporter::new(tee_sink(json), new_batch_id())
}

fn convert(a: ConvertArgs) -> Result<i32, EngineError> {
    let out = a
        .output
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("sdc-out"));
    let mut req = build_request(&a.batch, out).map_err(EngineError::Usage)?;
    req.jobs = a.jobs;
    req.mem_budget = a
        .mem_budget
        .as_deref()
        .map(parse_bytes)
        .transpose()
        .map_err(EngineError::Usage)?
        .unwrap_or(0);
    req.on_existing = a.on_existing.into();
    req.dry_run = a.dry_run;
    req.sidecar = a.sidecar;
    req.bc1_model = a.bc1_decoder.into();
    req.nvtt = a.nvtt.clone();
    // Before planning, so a Ctrl-C during a long scan is not lost. A failed install is not fatal, but it is
    // reported: without the handler Ctrl-C ends the process at once, as for any console program.
    let cancel = CancelToken::default();
    if let Err(e) = cancel::install(cancel.clone()) {
        err!(
            "warning: could not install the Ctrl-C handler ({e}); Ctrl-C will end sdc immediately"
        );
    }
    let engine = Engine::new()?;
    // Track A (Note CN): `[export] also` in the project `sdc.toml` writes the three folders under `-o`. A bad
    // `[export.legacy]` value is an error on this path only (O2): `run_also` checks it, `export_config` does not.
    let cfg = engine.export_config(&req)?;
    if !cfg.also.is_empty() {
        let spec = AlsoSpec {
            cfg,
            location: req.output_root.clone(),
            // The CLI reads no scan (TA13, TA18), so the env `_m` says "no scan".
            scanned: false,
            // shortcut: the CLI has no resolved PGP flags (TA27), so no set is marked inner-layer.
            inner_layer_sets: BTreeSet::new(),
        };
        // `run_also` returns Ok even when the PBR phase failed (CN56), so the exit code is the summary's.
        let sum = run_also(
            &engine,
            &AnalysisCache::default(),
            req,
            &spec,
            tee_sink(a.batch.json),
            &cancel,
        )?;
        return Ok(sum.exit_code());
    }
    let rep = reporter(a.batch.json);
    let plan = engine.plan(req, &rep, &cancel)?;
    Ok(engine.run(plan, &rep, &cancel)?.exit_code())
}

fn run(cli: Cli) -> Result<i32, EngineError> {
    match cli.cmd {
        Command::Convert(a) => convert(a),
        Command::Inspect(a) => verbs::inspect(a),
        Command::Validate(a) => verbs::validate_files(a),
        Command::Presets(c) => verbs::presets(c),
        Command::Encoders(a) => verbs::encoders(a),
        Command::PatchHeader(a) => verbs::patch_header(a),
        Command::CubemapMarker(a) => verbs::cubemap_marker(a),
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = e.print();
            return ExitCode::from(code);
        }
    };
    // An internal error (panic) is a fatal exit 3, not Rust's 101 (spec §8).
    match std::panic::catch_unwind(|| run(cli)) {
        Ok(Ok(code)) => ExitCode::from(code as u8),
        Ok(Err(e)) => {
            err!("sdc: {e}");
            ExitCode::from(e.exit_code() as u8)
        }
        Err(_) => {
            err!("sdc: internal error");
            ExitCode::from(3)
        }
    }
}
