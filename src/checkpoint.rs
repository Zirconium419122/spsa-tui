use std::{collections::HashMap, error::Error};

use serde::{Deserialize, Serialize};

use crate::params::{Param, ParamSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamHist {
    pub name: String,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub c_end: f64,
    pub r_end: f64,
    pub values: Vec<f64>,
    pub tune: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub engine: String,
    pub book: String,
    pub tc: String,

    pub total_iterations: usize,
    pub current_iteration: usize,

    pub wins: usize,
    pub draws: usize,
    pub losses: usize,

    pub time_iter: usize,
    pub time_samples: u32,

    pub params: Vec<ParamHist>,
}

impl Checkpoint {
    pub fn new(
        engine: String,
        book: String,
        tc: String,
        total_iterations: usize,
        params: &ParamSet,
    ) -> Checkpoint {
        let param_hist = params
            .iter()
            .map(|(name, p)| ParamHist {
                name: name.clone(),
                default: p.value,
                min: p.min,
                max: p.max,
                c_end: p.c_end,
                r_end: p.r_end,
                values: vec![p.value],
                tune: true,
            })
            .collect();

        Checkpoint {
            engine,
            book,
            tc,

            total_iterations,
            current_iteration: 1,

            wins: 0,
            draws: 0,
            losses: 0,

            time_iter: 0,
            time_samples: 0,

            params: param_hist,
        }
    }

    pub fn update(&mut self, params: &ParamSet, k: usize, wins: usize, draws: usize, losses: usize, time: usize) {
        for p in params {
            if let Some(hist) = self.params.iter_mut().find(|x| x.name == *p.0) {
                hist.values.push(p.1.value);
            }
        }
        self.wins = wins;
        self.draws = draws;
        self.losses = losses;
        self.total_iterations = self.total_iterations.max(k);
        self.current_iteration = k;

        const ALPHA: f64 = 0.2;
        if self.time_samples == 0 {
            self.time_iter = time;
        } else {
            self.time_iter = (self.time_iter as f64 * (1.0 - ALPHA) + time as f64 * ALPHA) as usize;
        }
        self.time_samples += 1;
    }

    pub fn write(&self) -> Result<(), Box<dyn Error>> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write("checkpoint.json", json)?;
        Ok(())
    }

    pub fn load() -> Result<Checkpoint, Box<dyn Error>> {
        let file = std::fs::read("checkpoint.json")?;
        Ok(serde_json::from_slice(&file)?)
    }

    pub fn param_hist_map(&self) -> HashMap<String, Vec<f64>> {
        self.params
            .iter()
            .map(|p| (p.name.clone(), p.values.clone()))
            .collect()
    }

    pub fn latest_params(&self) -> ParamSet {
        self.params
            .iter()
            .map(|p| {
                let value = *p.values.last().unwrap_or(&p.default);
                (
                    p.name.clone(),
                    Param {
                        value,
                        min: p.min,
                        max: p.max,
                        c_end: p.c_end,
                        r_end: p.r_end,
                    },
                )
            })
            .collect()
    }
}
