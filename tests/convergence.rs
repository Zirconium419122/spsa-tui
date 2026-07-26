use std::collections::HashMap;

use spsa_tui::params::{Param, ParamSet};
use spsa_tui::tune::{perturb, update};

fn goodness(
    cfg: &HashMap<String, f64>,
    ranges: &HashMap<String, f64>,
    targets: &HashMap<String, f64>,
) -> f64 {
    -cfg.iter()
        .map(|(name, v)| ((v - targets[name]) / ranges[name]).powi(2))
        .sum::<f64>()
}

fn synthetic_score(
    plus_options: &HashMap<String, isize>,
    minus_options: &HashMap<String, isize>,
    ranges: &HashMap<String, f64>,
    targets: &HashMap<String, f64>,
    tries: isize,
    sharpness: f64,
) -> (isize, isize) {
    let plus: HashMap<String, f64> = plus_options
        .iter()
        .map(|(n, v)| (n.clone(), *v as f64))
        .collect();
    let minus: HashMap<String, f64> = minus_options
        .iter()
        .map(|(n, v)| (n.clone(), *v as f64))
        .collect();

    let g_plus = goodness(&plus, ranges, targets);
    let g_minus = goodness(&minus, ranges, targets);

    let win_prob = 1.0 / (1.0 + (-(g_plus - g_minus) * sharpness).exp());

    let wins = (tries as f64 * win_prob).round() as isize;
    let losses = tries - wins;

    (wins, losses)
}

fn test_param(value: f64, min: f64, max: f64, r_end: f64) -> Param {
    Param {
        value,
        min,
        max,
        c_end: ((max - min).abs() * 0.05).max(1.0),
        r_end,
        tune: true,
    }
}

#[test]
fn spsa_converges_on_many_parameter_known_minimum() {
    let spec: &[(&str, f64, f64, f64)] = &[
        ("a", 0.0, 4000.0, 155.0),
        ("b", 0.0, 200.0, 40.0),
        ("c", -50.0, 250.0, 120.0),
        ("d", 0.0, 50.0, 43.0),
        ("e", 100.0, 300.0, 210.0),
        ("f", 0.0, 60.0, 12.0),
        ("g", 0.0, 20.0, 19.0),
        ("h", 500.0, 1000.0, 815.0),
        ("i", 0.0, 10.0, 0.0),
        ("j", 200.0, 250.0, 247.0),
        ("k", -100.0, -60.0, -85.0),
        ("l", 0.0, 640.0, 500.0),
    ];

    let mut targets: HashMap<String, f64> = HashMap::new();
    let mut ranges: HashMap<String, f64> = HashMap::new();
    let mut params: ParamSet = HashMap::new();
    for (name, min, max, target) in spec {
        targets.insert(name.to_string(), *target);
        ranges.insert(name.to_string(), max - min);
        let start = min + (max - min) * 0.3;
        params.insert(name.to_string(), test_param(start, *min, *max, 0.0001));
    }

    let total_iterations = 2000;
    let tries_per_iteration = 256;

    for k in 1..=total_iterations {
        let (deltas, plus_options, minus_options) = perturb(&params, k, total_iterations);

        let (wins, losses) = synthetic_score(
            &plus_options,
            &minus_options,
            &ranges,
            &targets,
            tries_per_iteration,
            10.0,
        );

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

#[test]
fn spsa_converges_on_synthetic_convex_function() {
    let mut targets: HashMap<String, f64> = HashMap::new();
    targets.insert("a".to_string(), 150.0);
    targets.insert("b".to_string(), 40.0);

    let mut ranges: HashMap<String, f64> = HashMap::new();
    ranges.insert("a".to_string(), 1.0);
    ranges.insert("b".to_string(), 1.0);

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

        let (wins, losses) = synthetic_score(
            &plus_options,
            &minus_options,
            &ranges,
            &targets,
            tries_per_iteration,
            0.0005,
        );

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
