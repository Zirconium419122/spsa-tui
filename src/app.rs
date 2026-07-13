use std::{
    collections::HashMap,
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::Receiver,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Direction, Layout},
    style::Style,
    symbols::Marker,
    text::Line,
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Row, Table, Widget},
};

use crate::{
    params::{ParamSet, build_param_set, find_latest_checkpoint, load_checkpoint_history},
    spsa::{Spsa, SpsaConfig, SpsaEvent},
};

pub struct App {
    engine: String,
    total_iterations: Arc<AtomicUsize>,
    save_iterations: usize,
    book: String,
    games_per_iter: usize,
    concurrency: usize,
    tc: String,

    param_hist: HashMap<String, Vec<f64>>,
    k: usize,
    wins: usize,
    draws: usize,
    losses: usize,
    time_iter: usize,

    spsa: Option<Arc<Mutex<Spsa>>>,
    spsa_handle: Option<JoinHandle<()>>,
    rx: Option<Receiver<SpsaEvent>>,

    running: Arc<AtomicBool>,
    paused: bool,
    exit: bool,
}

impl Widget for &App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        let [left, right] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
            .areas(area);

        let [left_top, left_bottom] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
            .areas(left);

        let [right_top, right_bottom] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
            .areas(right);

        let data: Vec<(String, Vec<(f64, f64)>)> = self
            .param_hist
            .iter()
            .map(|(name, param)| {
                (
                    name.clone(),
                    param
                        .iter()
                        .enumerate()
                        .map(|(i, x)| (i as f64, *x))
                        .collect::<Vec<(f64, f64)>>(),
                )
            })
            .collect();

        let datasets = data
            .iter()
            .map(|(name, data)| {
                Dataset::default()
                    .name(name.clone())
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Line)
                    .data(data)
            })
            .collect();

        let iterations = self.k.to_string();
        let x_axis = Axis::default()
            .title("Iterations")
            .bounds([0.0, self.k as f64])
            .labels(["0", &iterations]);

        let values: Vec<f64> = data
            .iter()
            .flat_map(|(_, x)| x.iter().map(|(_, v)| *v))
            .collect();
        let min = values
            .iter()
            .min_by(|a, b| a.total_cmp(b))
            .unwrap_or(&0.0)
            .ceil()
            - 5.0;
        let max = values
            .iter()
            .max_by(|a, b| a.total_cmp(b))
            .unwrap_or(&0.0)
            .floor()
            + 5.0;

        let min_str = format!("{:.2}", min);
        let max_str = format!("{:.2}", max);
        let y_axis = Axis::default()
            .title("Values")
            .bounds([min, max])
            .labels([min_str, max_str]);

        Chart::new(datasets)
            .x_axis(x_axis)
            .y_axis(y_axis)
            .legend_position(None)
            .block(Block::new().borders(Borders::ALL))
            .render(left_top, buf);

        let header = Row::new(["Name", "Value", "SD100", "SD500", "SDALL", "Delta"])
            .style(Style::new().bold())
            .bottom_margin(1);

        let widths = [
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ];

        macro_rules! standard_deviation {
            ($values:expr, $average:expr, $n:expr) => {{
                let values = $values.get($values.len().saturating_sub($n)..).unwrap();
                let s_sq = values
                    .iter()
                    .fold(0.0, |acc, x| acc + (x - $average).powi(2))
                    / (values.len() - 1) as f64;
                s_sq.sqrt()
            }};
            ($values:expr, $average:expr) => {
                standard_deviation!($values, $average, $values.len())
            };
        }

        let mut rows = Vec::new();
        for (name, hist) in &self.param_hist {
            let average = hist.iter().sum::<f64>() / hist.len() as f64;

            rows.push(Row::new([
                name.clone(),
                format!("{:.3}", hist.last().unwrap()),
                format!("{:.3}", standard_deviation!(hist, average, 100)),
                format!("{:.3}", standard_deviation!(hist, average, 500)),
                format!("{:.3}", standard_deviation!(hist, average)),
                format!("{:.3}", hist.last().unwrap() - hist[0]),
            ]));
        }

        Table::new(rows, widths)
            .header(header)
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title_top(Line::from(" Parameters ").left_aligned()),
            )
            .render(left_bottom, buf);

        let params_str = self
            .param_hist
            .iter()
            .map(|(name, vals)| {
                let val = vals.last().unwrap_or(&0.0);
                format!("{}: {:.3}", name, val)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let total = self.wins + self.losses;
        let score = if total > 0 {
            format!("{:.2}%", self.wins as f64 / total as f64 * 100.0)
        } else {
            "-".into()
        };
        let info = format!(
            "Iteration {}:\n\nWins:   {}\nDraws:  {}\nLosses: {}\nScore:  {}\n\n{}",
            self.k, self.wins, self.draws, self.losses, score, params_str
        );
        Paragraph::new(info)
            .left_aligned()
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(Line::from(" Iteration ").centered()),
            )
            .render(right_bottom, buf);

        let etc = (self.total_iterations.load(Ordering::Relaxed) - self.k) * self.time_iter / 1000;

        let settings = [
            format!("Engine           : {}", self.engine),
            format!("Book             : {}", self.book),
            format!("Total iterations : {}", self.total_iterations.load(Ordering::Relaxed)),
            format!("Games per iter   : {}", self.games_per_iter),
            format!("Concurrency      : {}", self.concurrency),
            format!("Time control     : {}", self.tc),
            "".into(),
            format!("Estimated time   : {:02}:{:02}:{:02}", etc / 3600, (etc % 3600) / 60, etc % 60),
        ]
        .join("\n");
        Paragraph::new(settings)
            .left_aligned()
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(Line::from(" Settings ").centered()),
            )
            .render(right_top, buf);
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> App {
        App {
            engine: "./ferrischess".into(),
            total_iterations: Arc::new(AtomicUsize::new(2000)),
            save_iterations: 10,
            book: "../8moves_v3.pgn".into(),
            games_per_iter: 16,
            concurrency: 8,
            tc: "10+0.1".into(),

            param_hist: HashMap::new(),
            k: 1,
            wins: 0,
            draws: 0,
            losses: 0,
            time_iter: 0,

            spsa: None,
            spsa_handle: None,
            rx: None,

            running: Arc::new(AtomicBool::new(false)),
            paused: true,
            exit: false,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<(), Box<dyn Error>> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;

            if let Some(rx) = &self.rx
                && let Ok(event) = rx.try_recv()
            {
                match event {
                    SpsaEvent::Iteration {
                        params,
                        k,
                        wins,
                        draws,
                        losses,
                        time,
                    } => {
                        for p in params {
                            self.param_hist.entry(p.0).or_default().push(p.1.value);
                        }
                        self.k = k;
                        self.wins = wins;
                        self.draws = draws;
                        self.losses = losses;
                        self.time_iter = (self.time_iter + time) / 2;
                    }
                    SpsaEvent::Error(e) => return Err(e),
                }
            }

            if !self.exit
                && !self.paused
                && !self.running.load(Ordering::Relaxed)
                && let Some(spsa) = &self.spsa
            {
                self.running.store(true, Ordering::Relaxed);

                let spsa = spsa.clone();
                let running = self.running.clone();

                self.spsa_handle = Some(thread::spawn(move || {
                    spsa.lock().unwrap().next();
                    running.store(false, Ordering::Relaxed);
                }));
            }

            if self.exit {
                break Ok(());
            }
        }
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> Result<(), Box<dyn Error>> {
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    self.handle_key_event(key_event)
                }
                _ => {}
            };
        }

        Ok(())
    }

    fn handle_key_event(&mut self, key_event: event::KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),
            KeyCode::Char(' ') => self.paused = !self.paused,
            KeyCode::Char('r') => self.resume_from_checkpoint(),
            KeyCode::Enter => {
                let params = match build_param_set(&self.engine) {
                    Ok(v) => v,
                    Err(_) => return,
                };

                for p in &params {
                    self.param_hist
                        .entry(p.0.clone())
                        .or_default()
                        .push(p.1.value);
                }

                let config = SpsaConfig {
                    engine: self.engine.clone(),
                    total_iterations: self.total_iterations.clone(),
                    save_iterations: self.save_iterations,
                    params,
                    start_k: 1,
                    book: self.book.clone(),
                    games_per_iter: self.games_per_iter,
                    concurrency: self.concurrency,
                    tc: self.tc.clone(),
                };

                let (tx, rx) = std::sync::mpsc::channel::<SpsaEvent>();
                self.rx = Some(rx);
                self.spsa = Some(Arc::new(Mutex::new(Spsa::new(
                    config,
                    tx,
                    self.running.clone(),
                ))));

                self.paused = false;
            }
            KeyCode::Up => {
                self.total_iterations.fetch_add(100, Ordering::Relaxed);
            }
            KeyCode::Down => {
                let previous = self.total_iterations.fetch_sub(100, Ordering::Relaxed);
                if previous <= 100 {
                    self.total_iterations.store(1, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }

    fn resume_from_checkpoint(&mut self) {
        let Some(latest_iteration) = find_latest_checkpoint() else {
            return;
        };

        let Ok(checkpoint_history) = load_checkpoint_history() else {
            return;
        };

        let latest_params = checkpoint_history.last().map(|(_, p)| p.clone()).unwrap();

        self.param_hist.clear();

        let mut prev_iter = 0;
        let mut prev_params: Option<&ParamSet> = None;

        for (cur_iter, cur_params) in &checkpoint_history {
            let gap = cur_iter.saturating_sub(prev_iter).max(1);

            for (name, cur_param) in cur_params {
                let prev_value = prev_params
                    .and_then(|params| params.get(name))
                    .map_or(cur_param.value, |p| p.value);

                let entry = self.param_hist.entry(name.clone()).or_default();
                for i in 1..=gap {
                    let t = i as f64 / gap as f64;
                    entry.push(prev_value + (cur_param.value - prev_value) * t);
                }
            }

            prev_iter = *cur_iter;
            prev_params = Some(cur_params);
        }

        self.k = latest_iteration + 1;

        let config = SpsaConfig {
            engine: self.engine.clone(),
            total_iterations: self.total_iterations.clone(),
            save_iterations: self.save_iterations,
            params: latest_params,
            start_k: self.k,
            book: self.book.clone(),
            games_per_iter: self.games_per_iter,
            concurrency: self.concurrency,
            tc: self.tc.clone(),
        };

        let (tx, rx) = std::sync::mpsc::channel::<SpsaEvent>();
        self.rx = Some(rx);
        self.spsa = Some(Arc::new(Mutex::new(Spsa::new(
            config,
            tx,
            self.running.clone(),
        ))));
    }

    fn exit(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        self.exit = true;

        if let Some(spsa_handle) = self.spsa_handle.take() {
            let _ = spsa_handle.join();
        }
    }
}
