use std::{collections::HashMap, error::Error};

use serde::{Deserialize, Serialize};

use crate::uci::get_tunable_params;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Param {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub c_end: f64,
    pub r_end: f64,
}

pub type ParamSet = HashMap<String, Param>;

pub fn build_param_set(engine: &str) -> Result<ParamSet, Box<dyn Error>> {
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

pub fn save_checkpoint(params: &ParamSet, iteration: usize) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all("checkpoints")?;

    let path = format!("checkpoints/spsa_checkpoint_{}.json", iteration);
    let json = serde_json::to_string_pretty(params)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_checkpoint(iteration: usize) -> Result<ParamSet, Box<dyn Error>> {
    let path = format!("checkpoints/spsa_checkpoint_{}.json", iteration);
    let file = std::fs::read(path)?;
    Ok(serde_json::from_slice(&file)?)
}

pub fn find_latest_checkpoint() -> Option<usize> {
    let dir = std::fs::read_dir("checkpoints").ok()?;

    let mut latest = None;

    for entry in dir.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };

        if let Some(number) = name
            .strip_prefix("spsa_checkpoint_")
            .and_then(|s| s.strip_suffix(".json"))
            && let Ok(number) = number.parse::<usize>()
        {
            latest = Some(latest.map_or(number, |latest: usize| latest.max(number)));
        }
    }

    latest
}

pub fn load_checkpoint_history() -> Result<Vec<(usize, ParamSet)>, Box<dyn Error>> {
    let Some(total_iterations) = find_latest_checkpoint() else {
        return Err("No checkpoints found".into());
    };

    let mut checkpoints = Vec::new();

    for iteration in 1..=total_iterations {
        if let Ok(params) = load_checkpoint(iteration) {
            checkpoints.push((iteration, params));
        }
    }

    Ok(checkpoints)
}
