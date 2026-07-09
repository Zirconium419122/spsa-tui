use std::{collections::HashMap, io::Error};

use serde::Serialize;

use crate::uci::get_tunable_params;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Param {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub c_end: f64,
    pub r_end: f64,
}

pub type ParamSet = HashMap<String, Param>;

pub fn build_param_set(engine: &str) -> Result<ParamSet, Error> {
    let options = get_tunable_params(engine)?;

    let mut params = HashMap::new();
    for option in options {
        let c_end = ((option.max.unwrap() - option.min.unwrap()) as f64 * 0.05).max(1.0);

        params.insert(
            option.name,
            Param {
                value: option.default.unwrap().parse::<isize>().unwrap() as f64,
                min: option.min.unwrap() as f64,
                max: option.max.unwrap() as f64,
                c_end,
                r_end: 0.002,
            },
        );
    }

    Ok(params)
}

pub fn save_checkpoint(params: &ParamSet, iteration: usize) -> Result<(), Error> {
    std::fs::create_dir_all("checkpoints")?;

    let path = format!("checkpoints/spsa_checkpoint_{}.json", iteration);
    let json = serde_json::to_string_pretty(params)?;
    std::fs::write(path, json)
}
