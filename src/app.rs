use std::{
    collections::HashMap,
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
    },
    thread,
};

use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Direction, Layout},
    symbols::{Marker, border},
    text::Line,
    widgets::{Axis, Block, Chart, Dataset, GraphType, Paragraph, Widget},
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
    losses: usize,

    spsa: Option<Spsa>,
    rx: Option<Receiver<SpsaEvent>>,

    done: Arc<AtomicBool>,
    exit: bool,
}

impl Widget for &App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        let [left, right] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
            .areas(area);
        let title = Line::from("SPSA tuner").centered();

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
            .unwrap_or(0.0);
        let max = data
            .iter()
            .flat_map(|(_, x)| x)
            .map(|(_, x)| *x)
            .max_by(|a, b| a.total_cmp(b))
            .unwrap_or(1.0);

        let min_str = format!("{:.3}", min);
        let max_str = format!("{:.3}", max);
        let y_axis = Axis::default()
            .title("Values")
            .bounds([min, max])
            .labels([min_str, max_str]);

        Chart::new(datasets)
            .x_axis(x_axis)
            .y_axis(y_axis)
            .render(left, buf);

        let block = Block::bordered().title(title).border_set(border::THICK);

        let info = format!("Iteration {}:", self.k);
        Paragraph::new(info)
            .left_aligned()
            .block(block)
            .render(right, buf);
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
            losses: 0,
            wins: 0,

            spsa: None,
            rx: None,

            done: Arc::new(AtomicBool::new(false)),
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
                        losses,
                    } => {
                        for p in params {
                            if let Some(val) = self.param_hist.get_mut(&p.0) {
                                val.push(p.1.value);
                            };
                        }
                        self.k = k;
                        self.wins = wins;
                        self.losses = losses;
                    }
                    SpsaEvent::Error(e) => return Err(e),
                }
            }

            if !self.exit
                && !self.done.load(Ordering::Relaxed)
                && let Some(spsa) = &self.spsa
            {
                let mut spsa = spsa.clone();
                let done = self.done.clone();

                thread::spawn(move || match spsa.next() {
                    Some(Ok(())) => {}
                    Some(Err(e)) => {
                        let _ = spsa.send(SpsaEvent::Error(e));
                    }
                    None => done.store(true, Ordering::Relaxed),
                });
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
            KeyCode::Enter => {
                let params = match build_param_set(&self.engine) {
                    Ok(v) => v,
                    Err(_) => return,
                };

                for p in &params {
                    if let Some(val) = self.param_hist.get_mut(p.0) {
                        val.push(p.1.value);
                    };
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
                self.spsa = Some(Spsa::new(config, tx));
            }
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}
