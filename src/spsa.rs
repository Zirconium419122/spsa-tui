use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::Sender,
    },
    time::Instant,
};

use crate::{
    fastchess::{MatchConfig, run_match},
    params::ParamSet,
    tune::{perturb, update},
};

pub struct SpsaConfig {
    pub engine: String,
    pub total_iterations: Arc<AtomicUsize>,
    pub start_k: usize,
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
        draws: usize,
        losses: usize,
        time: usize,
    },
    Error(Box<dyn Error>),
}

unsafe impl Send for SpsaEvent {}

#[derive(Clone)]
pub struct Spsa {
    total_iterations: Arc<AtomicUsize>,
    params: ParamSet,
    match_config: MatchConfig,
    k: usize,
    tx: Sender<SpsaEvent>,
    running: Arc<AtomicBool>,
}

impl Iterator for Spsa {
    type Item = ();

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done() {
            return None;
        }

        let start = Instant::now();

        let total_iterations = self.total_iterations.load(Ordering::Relaxed);

        let (deltas, plus_options, minus_options) = perturb(&self.params, self.k, total_iterations);

        let (wins, draws, losses) = match run_match(
            &self.match_config,
            &plus_options,
            &minus_options,
            self.running.clone(),
        ) {
            Ok(v) => v,
            Err(e) => {
                let _ = self.send(SpsaEvent::Error(e));
                return Some(());
            }
        };

        update(&mut self.params, &deltas, wins, losses);

        let _ = self.send(SpsaEvent::Iteration {
            params: self.params.clone(),
            k: self.k,
            wins: wins as usize,
            draws: draws as usize,
            losses: losses as usize,
            time: start.elapsed().as_millis() as usize,
        });

        self.k += 1;

        Some(())
    }
}

impl Spsa {
    pub fn new(config: SpsaConfig, tx: Sender<SpsaEvent>, running: Arc<AtomicBool>) -> Spsa {
        let match_config = MatchConfig {
            engine: config.engine,
            tc: config.tc,
            book: config.book,
            games: config.games_per_iter,
            concurrency: config.concurrency,
        };

        Spsa {
            total_iterations: config.total_iterations,
            params: config.params,
            match_config,
            k: config.start_k,
            tx,
            running,
        }
    }

    pub fn send(&self, event: SpsaEvent) -> Result<(), Box<dyn Error>> {
        self.tx.send(event)?;
        Ok(())
    }

    pub fn set_param_enabled(&mut self, name: &str, enabled: bool) {
        if let Some(p) = self.params.get_mut(name) {
            p.tune = enabled;
        }
    }

    fn is_done(&self) -> bool {
        self.k > self.total_iterations.load(Ordering::Relaxed)
    }
}
