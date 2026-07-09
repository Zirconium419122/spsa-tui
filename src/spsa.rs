use std::{error::Error, sync::mpsc::Sender};

use crate::{
    fastchess::{MatchConfig, run_match},
    params::{ParamSet, save_checkpoint},
    tune::{perturb, update},
};

pub struct SpsaConfig {
    pub engine: String,
    pub total_iterations: usize,
    pub save_iterations: usize,
    pub params: ParamSet,
    pub book: String,
    pub games_per_iter: usize,
    pub concurrency: usize,
    pub tc: String,
}

pub enum SpsaEvent {
    Iteration {
        params: ParamSet,
        k: usize,
        wins: usize,
        losses: usize,
    },
    Error(Box<dyn Error>),
}

unsafe impl Send for SpsaEvent {}

#[derive(Clone)]
pub struct Spsa {
    total_iterations: usize,
    save_iterations: usize,
    params: ParamSet,
    match_config: MatchConfig,
    k: usize,
    tx: Sender<SpsaEvent>,
}

impl Iterator for Spsa {
    type Item = Result<(), Box<dyn Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done() {
            return None;
        }

        let (deltas, plus_options, minus_options) =
            perturb(&self.params, self.k, self.total_iterations);

        let (wins, losses) = match run_match(&self.match_config, &plus_options, &minus_options) {
            Ok(v) => v,
            Err(e) => return Some(Err(e.into())),
        };

        update(&mut self.params, &deltas, wins, losses);

        if self.k % self.save_iterations == 0 || self.k == self.total_iterations {
            save_checkpoint(&self.params, self.k).unwrap();
        }

        match self.send(SpsaEvent::Iteration {
            params: self.params.clone(),
            k: self.k,
            wins: wins as usize,
            losses: losses as usize,
        }) {
            Ok(()) => {}
            Err(e) => return Some(Err(e.into())),
        }

        self.k += 1;

        Some(Ok(()))
    }
}

impl Spsa {
    pub fn new(config: SpsaConfig, tx: Sender<SpsaEvent>) -> Spsa {
        let match_config = MatchConfig {
            engine: config.engine,
            tc: config.tc,
            book: config.book,
            games: config.games_per_iter,
            concurrency: config.concurrency,
        };

        Spsa {
            total_iterations: config.total_iterations,
            save_iterations: config.save_iterations,
            params: config.params,
            match_config,
            k: 1,
            tx,
        }
    }

    pub fn send(&self, event: SpsaEvent) -> Result<(), Box<dyn Error>> {
        self.tx.send(event)?;
        Ok(())
    }

    fn is_done(&self) -> bool {
        self.k > self.total_iterations
    }
}
