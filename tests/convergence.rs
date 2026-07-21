use std::collections::HashMap;

use spsa_tui::params::{Param, ParamSet};
use spsa_tui::tune::{perturb, update};

fn goodness(cfg: &HashMap<String, f64>, targets: &HashMap<String, f64>) -> f64 {
    -cfg.iter()
        .map(|(name, v)| (v - targets[name]).powi(2))
        .sum::<f64>()
}

fn synthetic_score(
    plus_options: &HashMap<String, isize>,
    minus_options: &HashMap<String, isize>,
    targets: &HashMap<String, f64>,
    tries: isize,
) -> (isize, isize) {
    let plus: HashMap<String, f64> = plus_options
        .iter()
        .map(|(n, v)| (n.clone(), *v as f64))
        .collect();
    let minus: HashMap<String, f64> = minus_options
        .iter()
        .map(|(n, v)| (n.clone(), *v as f64))
        .collect();

    let g_plus = goodness(&plus, targets);
    let g_minus = goodness(&minus, targets);

    let win_prob = 1.0 / (1.0 + (-(g_plus - g_minus) * 0.0005).exp());

    let wins = (tries as f64 * win_prob).round() as isize;
    let losses = tries - wins;

    (wins, losses)
}

#[test]
fn spsa_converges_on_synthetic_convex_function() {
    let mut targets: HashMap<String, f64> = HashMap::new();
    targets.insert("a".to_string(), 150.0);
    targets.insert("b".to_string(), 40.0);

    let mut params: ParamSet = HashMap::new();
    params.insert(
        "a".to_string(),
        Param {
            value: 50.0,
            min: 0.0,
            max: 300.0,
            c_end: 15.0,
            r_end: 0.002,
            tune: true,
        },
    );
    params.insert(
        "b".to_string(),
        Param {
            value: 90.0,
            min: 0.0,
            max: 150.0,
            c_end: 7.5,
            r_end: 0.002,
            tune: true,
        },
    );

    let total_iterations = 2000;
    let tries_per_iteration = 16;

    for k in 1..=total_iterations {
        let (deltas, plus_options, minus_options) = perturb(&params, k, total_iterations);

        let (wins, losses) =
            synthetic_score(&plus_options, &minus_options, &targets, tries_per_iteration);

        update(&mut params, &deltas, wins, losses);

        if k % 100 == 0 {
            println!("Iteration {k}:");
            for (name, p) in &params {
                println!("  {name} = {:.3} (target {:.3})", p.value, targets[name]);
            }
        }
    }

    for (name, p) in &params {
        let target = targets[name];
        let error = (p.value - target).abs();
        let tolerance = (p.max - p.min) * 0.05;

        println!(
            "Final {name}: value={:.2} target={:.2} error={:.2} tolerance={:.2}",
            p.value, target, error, tolerance
        );
        assert!(
            error < tolerance,
            "{name} failed to converge: value={:.2}, target={:.2}, tolerance={:.2}",
            p.value,
            target,
            tolerance
        );
    }
}
