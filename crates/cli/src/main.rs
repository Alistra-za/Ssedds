//! `sdc` command line (spec §8). Human output goes to stderr; with `--json`, stdout carries only JSON Lines.
//! Exit codes, highest precedence first: 3 fatal > 2 usage/config > 1 partial > 4 validation failed > 0.
mod args;
mod human;
mod verbs;

use args::*;
use clap::Parser;
use engine::api::{CancelToken, Engine, EngineError};
use engine::events::{new_batch_id, EventSink, JsonLinesSink, Reporter};
use std::process::ExitCode;
use std::sync::Arc;

fn reporter(json: bool) -> Reporter {
    let mut sinks: Vec<Arc<dyn EventSink>> = vec![Arc::new(human::HumanSink::new())];
    if json {
        sinks.push(Arc::new(JsonLinesSink::new(std::io::stdout())));
    }
    Reporter::new(Arc::new(human::TeeSink(sinks)), new_batch_id())
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
    if a.nvtt.is_some() {
        eprintln!("note: --nvtt is accepted but the NVTT backend arrives in milestone M5");
    }
    let rep = reporter(a.batch.json);
    let engine = Engine::new()?;
    let plan = engine.plan(req, &rep)?;
    let cancel = CancelToken::default();
    let c2 = cancel.clone();
    let _ = ctrlc::set_handler(move || {
        eprintln!("cancelling: finishing current bands...");
        c2.cancel();
    });
    Ok(engine.run(plan, &rep, &cancel)?.exit_code())
}

fn run(cli: Cli) -> Result<i32, EngineError> {
    match cli.cmd {
        Command::Convert(a) => convert(a),
        Command::Inspect(a) => verbs::inspect(a),
        Command::Validate(a) => verbs::validate_files(a),
        _ => Err(EngineError::Usage(
            "this command is not implemented yet".into(),
        )),
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
            eprintln!("sdc: {e}");
            ExitCode::from(e.exit_code() as u8)
        }
        Err(_) => {
            eprintln!("sdc: internal error");
            ExitCode::from(3)
        }
    }
}
