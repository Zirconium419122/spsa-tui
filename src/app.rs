use std::{
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::Receiver,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use ratatui::{
    DefaultTerminal, Frame,
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
        execute,
    },
    layout::{Constraint, Direction, Layout},
    style::Style,
    symbols::Marker,
    text::Line,
    widgets::{
        Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Row, Table, TableState, Widget,
    },
};

use crate::{
    checkpoint::{Checkpoint, ParamHist},
    params::build_param_set,
    spsa::{Spsa, SpsaConfig, SpsaEvent},
    tune::{a_k, c_k},
};

#[derive(PartialEq)]
enum Mode {
    Normal,
    ParamSelect,
}

pub struct App {
    engine: String,
    total_iterations: Arc<AtomicUsize>,
    save_iterations: usize,
    book: String,
    games_per_iter: usize,
    concurrency: usize,
    tc: String,

    spsa: Option<Arc<Mutex<Spsa>>>,
    spsa_handle: Option<JoinHandle<()>>,
    rx: Option<Receiver<SpsaEvent>>,

    checkpoint: Option<Checkpoint>,

    selected_param_idx: usize,
    mode: Mode,
    table_state: TableState,

    running: Arc<AtomicBool>,
    run_timer: Option<Instant>,
    paused: bool,
    exit: bool,
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

            spsa: None,
            spsa_handle: None,
            rx: None,

            checkpoint: None,

            running: Arc::new(AtomicBool::new(false)),
            run_timer: None,
            paused: true,
            exit: false,

            mode: Mode::Normal,
            selected_param_idx: 0,
            table_state: TableState::default(),
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<(), Box<dyn Error>> {
        execute!(std::io::stdout(), EnableMouseCapture)?;

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
                        if let Some(checkpoint) = &mut self.checkpoint {
                            checkpoint.update(&params, k, wins, draws, losses, time);

                            if k.is_multiple_of(self.save_iterations)
                                || k == self.total_iterations.load(Ordering::Relaxed)
                            {
                                checkpoint.write().unwrap();
                            }
                        }
                    }
                    SpsaEvent::Error(e) => return Err(e),
                }
            }

            if !self.running.load(Ordering::Relaxed) {
                self.run_timer = None;
            }

            if !self.exit
                && !self.paused
                && !self.running.load(Ordering::Relaxed)
                && let Some(spsa) = &self.spsa
            {
                if let Some(checkpoint) = &self.checkpoint
                    && let Ok(mut spsa) = spsa.lock()
                {
                    for p in &checkpoint.params {
                        spsa.set_param_enabled(&p.name, p.tune);
                    }
                }

                self.running.store(true, Ordering::Relaxed);
                self.run_timer = Some(Instant::now());

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

    fn draw(&mut self, frame: &mut Frame) {
        let current_k = self.checkpoint.as_ref().map_or(0, |c| c.current_iteration);
        let wins = self.checkpoint.as_ref().map_or(0, |c| c.wins);
        let draws = self.checkpoint.as_ref().map_or(0, |c| c.draws);
        let losses = self.checkpoint.as_ref().map_or(0, |c| c.losses);
        let time_iter = self.checkpoint.as_ref().map_or(0, |c| c.time_iter);

        let [left, right] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
            .areas(frame.area());

        let [left_top, left_bottom] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
            .areas(left);

        let [right_top, right_bottom] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
            .areas(right);

        let param_list: Vec<&ParamHist> = self
            .checkpoint
            .as_ref()
            .map(|c| c.params.iter().collect())
            .unwrap_or_default();

        let data: Vec<(String, Vec<(f64, f64)>)> = param_list
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    p.values
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

        let iterations = current_k.to_string();
        let x_axis = Axis::default()
            .title("Iterations")
            .bounds([0.0, current_k as f64])
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
            .render(left_top, frame.buffer_mut());

        let header = Row::new([
            "Name", "Value", "SD100", "SD500", "SDALL", "Delta", "C_K", "A_K", "Tune",
        ])
        .style(Style::new().bold())
        .bottom_margin(1);

        let widths = [
            Constraint::Percentage(15),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
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
        for p in param_list.iter() {
            let average = p.values.iter().sum::<f64>() / p.values.len() as f64;

            let total_iterations = self.total_iterations.load(Ordering::Relaxed);
            rows.push(Row::new([
                p.name.clone(),
                format!("{:.3}", p.values.last().unwrap()),
                format!("{:.3}", standard_deviation!(p.values, average, 100)),
                format!("{:.3}", standard_deviation!(p.values, average, 500)),
                format!("{:.3}", standard_deviation!(p.values, average)),
                format!("{:.3}", p.values.last().unwrap() - p.values[0]),
                format!("{:.3}", c_k(p.c_end, current_k, total_iterations)),
                format!("{:.3}", a_k(p.r_end, p.c_end, current_k, total_iterations)),
                if p.tune { "✓".into() } else { "✗".into() },
            ]));
        }

        let title = match self.mode {
            Mode::Normal => " Parameters ",
            Mode::ParamSelect => " Parameters [select] ",
        };

        if self.mode == Mode::ParamSelect {
            self.table_state.select(Some(self.selected_param_idx));
        } else {
            self.table_state.select(None);
        }

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(Style::new().reversed())
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title_top(Line::from(title).left_aligned()),
            );
        frame.render_stateful_widget(table, left_bottom, &mut self.table_state);

        let params_str = param_list
            .iter()
            .map(|p| {
                let val = p.values.last().unwrap_or(&0.0);
                format!("{}: {:.3}", p.name, val)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let total = wins + losses;
        let score = if total > 0 {
            format!("{:.2}%", wins as f64 / total as f64 * 100.0)
        } else {
            "-".into()
        };
        let info = [
            format!("Wins   : {}", wins),
            format!("Draws  : {}", draws),
            format!("Losses : {}", losses),
            format!("Score  : {}", score),
            "".into(),
            params_str,
        ]
        .join("\n");
        Paragraph::new(info)
            .left_aligned()
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(Line::from(format!(" Iteration {} ", current_k)).centered()),
            )
            .render(right_bottom, frame.buffer_mut());

        let etc = ((self.total_iterations.load(Ordering::Relaxed).saturating_sub(current_k)) * time_iter / 1000)
            .saturating_sub(self.run_timer.map_or(0, |x| x.elapsed().as_secs() as usize));

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
            .render(right_top, frame.buffer_mut());
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
        match self.mode {
            Mode::Normal => self.handle_normal_key(key_event),
            Mode::ParamSelect => self.handle_param_select_key(key_event),
        }
    }

    fn handle_normal_key(&mut self, key_event: event::KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),
            KeyCode::Char(' ') => {
                self.paused = !self.paused;
                if self.paused {
                    self.run_timer = None;
                }
            }
            KeyCode::Char('r') => self.resume_from_checkpoint(),
            KeyCode::Tab => {
                self.mode = Mode::ParamSelect;
            }
            KeyCode::Enter => {
                let params = match build_param_set(&self.engine) {
                    Ok(v) => v,
                    Err(_) => return,
                };

                let total_iter = self.total_iterations.load(Ordering::Relaxed);
                self.checkpoint = Some(Checkpoint::new(
                    self.engine.clone(),
                    self.book.clone(),
                    self.tc.clone(),
                    total_iter,
                    &params,
                ));
                self.selected_param_idx = 0;

                let config = SpsaConfig {
                    engine: self.engine.clone(),
                    total_iterations: self.total_iterations.clone(),
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

    fn handle_param_select_key(&mut self, key_event: event::KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),
            KeyCode::Tab | KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(checkpoint) = &self.checkpoint
                    && !checkpoint.params.is_empty()
                {
                    self.selected_param_idx = (self.selected_param_idx + checkpoint.params.len()
                        - 1)
                        % checkpoint.params.len();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(checkpoint) = &self.checkpoint
                    && !checkpoint.params.is_empty()
                {
                    self.selected_param_idx =
                        (self.selected_param_idx + 1) % checkpoint.params.len();
                }
            }
            KeyCode::Char(' ') => {
                if let Some(checkpoint) = &mut self.checkpoint
                    && let Some(p) = checkpoint.params.get_mut(self.selected_param_idx)
                {
                    p.tune = !p.tune;
                }
            }
            _ => {}
        }
    }

    fn resume_from_checkpoint(&mut self) {
        let Ok(checkpoint) = Checkpoint::load() else {
            return;
        };

        let start_k = checkpoint.current_iteration + 1;
        self.checkpoint = Some(checkpoint.clone());
        self.selected_param_idx = 0;

        let config = SpsaConfig {
            engine: self.engine.clone(),
            total_iterations: self.total_iterations.clone(),
            params: checkpoint.latest_params(),
            start_k,
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

        let _ = execute!(std::io::stdout(), DisableMouseCapture);

        if let Some(spsa_handle) = self.spsa_handle.take() {
            let _ = spsa_handle.join();
        }

        if let Some(checkpoint) = &self.checkpoint {
            let _ = checkpoint.write();
        }
    }
}
