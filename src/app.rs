use std::{
    collections::HashMap,
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Direction, Layout},
    symbols::Marker,
    text::Line,
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Widget},
};

use crate::{
    params::build_param_set,
    spsa::{Spsa, SpsaConfig, SpsaEvent},
};

pub struct App {
    engine: String,
    total_iterations: usize,
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

        // Fix this later i guess...
        let min = data
            .iter()
            .flat_map(|(_, x)| x)
            .map(|(_, x)| *x)
            .min_by(|a, b| a.total_cmp(b))
            .unwrap_or(0.0)
            .ceil()
            - 5.0;
        let max = data
            .iter()
            .flat_map(|(_, x)| x)
            .map(|(_, x)| *x)
            .max_by(|a, b| a.total_cmp(b))
            .unwrap_or(0.0)
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

        Paragraph::new("left_bottom")
            .block(Block::new().borders(Borders::ALL))
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
                    .title(Line::from(" SPSA tuner ").centered()),
            )
            .render(right_bottom, buf);

        Block::new()
            .borders(Borders::ALL)
            .title(Line::from(" Settings ").centered())
            .render(right_top, buf);
    }
}

impl App {
    pub fn new() -> App {
        App {
            engine: "./ferrischess".into(),
            total_iterations: 2000,
            save_iterations: 10,
            book: "../8moves_v3.pgn".into(),
            games_per_iter: 64,
            concurrency: 8,
            tc: "1+0.01".into(),

            param_hist: HashMap::new(),
            k: 1,
            wins: 0,
            draws: 0,
            losses: 0,

            spsa: None,
            spsa_handle: None,
            rx: None,

            running: Arc::new(AtomicBool::new(false)),
            paused: false,
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
                    } => {
                        for p in params {
                            self.param_hist.entry(p.0).or_default().push(p.1.value);
                        }
                        self.k = k;
                        self.wins = wins;
                        self.draws = draws;
                        self.losses = losses;
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
                    if spsa.lock().unwrap().next().is_some() {
                        running.store(false, Ordering::Relaxed)
                    }
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
                    total_iterations: self.total_iterations,
                    save_iterations: self.save_iterations,
                    params,
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
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        self.exit = true;

        if let Some(spsa_handle) = self.spsa_handle.take() {
            let _ = spsa_handle.join();
        }
    }
}
