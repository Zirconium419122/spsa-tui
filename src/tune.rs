use std::collections::HashMap;

use rand::RngExt;

use crate::params::{Param, ParamSet};

const ALPHA: f64 = 0.602;
const GAMMA: f64 = 0.101;

fn gain_sequences(k: usize, total_iterations: usize, param: &Param) -> (f64, f64) {
    let a_stab = total_iterations as f64 * 0.1;

    let c_k = param.c_end * (total_iterations as f64).powf(GAMMA) / (k as f64).powf(GAMMA);

    let a_end = param.r_end * param.c_end.powi(2);
    let a_k = a_end * (a_stab + total_iterations as f64).powf(ALPHA)
        / (a_stab + k as f64).powf(ALPHA)
        / c_k.powi(2);

    (a_k, c_k)
}

pub struct Perturbation {
    delta: f64,
    c_k: f64,
    a_k: f64,
}

pub fn perturb(
    params: &ParamSet,
    k: usize,
    total_iterations: usize,
) -> (
    HashMap<String, Perturbation>,
    HashMap<String, isize>,
    HashMap<String, isize>,
) {
    let mut rng = rand::rng();
    let mut deltas = HashMap::new();
    let mut plus_options = HashMap::new();
    let mut minus_options = HashMap::new();

    for (name, p) in params {
        if p.tune {
            let (a_k, c_k) = gain_sequences(k, total_iterations, p);
            let delta = if rng.random_bool(0.5) { 1.0 } else { -1.0 };

            let plus_value = (p.value + c_k * delta).clamp(p.min, p.max);
            let minus_value = (p.value - c_k * delta).clamp(p.min, p.max);

            plus_options.insert(
                name.clone(),
                (plus_value + rng.random::<f64>()).floor() as isize,
            );
            minus_options.insert(
                name.clone(),
                (minus_value + rng.random::<f64>()).floor() as isize,
            );

            deltas.insert(name.clone(), Perturbation { delta, c_k, a_k });
        } else {
            let value = (p.value + rng.random::<f64>()).floor() as isize;
            plus_options.insert(name.clone(), value);
            minus_options.insert(name.clone(), value);
        }
    }

    (deltas, plus_options, minus_options)
}

pub fn update(
    params: &mut ParamSet,
    deltas: &HashMap<String, Perturbation>,
    wins: isize,
    losses: isize,
) {
    let score = (wins - losses) as f64;

    for (name, p) in params.iter_mut() {
        if let Some(d) = deltas.get(name) {
            let increment = d.a_k * d.c_k * score * d.delta;
            p.value = (p.value + increment).clamp(p.min, p.max);
        }
    }
}
