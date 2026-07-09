use spsa_tui::{
    fastchess::{MatchConfig, run_match},
    params::{build_param_set, save_checkpoint},
    tune::{perturb, update},
};

fn main() {
    let engine = "./ferrischess";
    let mut params = build_param_set(engine).unwrap();

    println!("Found {} tunable parameters:", params.len());
    for (name, p) in &params {
        println!(
            "  {}: default={} min={} max={}",
            name, p.value, p.min, p.max,
        );
    }
    println!();

    let config = MatchConfig {
        engine: engine.into(),
        tc: "1+0.01".into(),
        book: "../8moves_v3.pgn".into(),
        games: 64,
        concurrency: 8,
    };

    let total_iterations = 2000;

    for k in 1..total_iterations {
        let (deltas, plus_options, minus_options) = perturb(&params, k, total_iterations);

        let (wins, losses) = run_match(&config, &plus_options, &minus_options).unwrap();

        update(&mut params, &deltas, wins, losses);

        if k % 10 == 0 {
            println!("\nIteration {}:", k);
            for (name, p) in &params {
                println!("  {} = {:.3}", name, p.value);
            }

            save_checkpoint(&params, k).unwrap();
        }
    }

    save_checkpoint(&params, total_iterations).unwrap();
}
