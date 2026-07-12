use std::{
    collections::HashMap,
    error::Error,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[derive(Clone)]
pub struct MatchConfig {
    pub engine: String,
    pub tc: String,
    pub book: String,
    pub games: usize,
    pub concurrency: usize,
}

pub fn run_match(
    config: &MatchConfig,
    plus_options: &HashMap<String, isize>,
    minus_options: &HashMap<String, isize>,
    running: Arc<AtomicBool>,
) -> Result<(isize, isize, isize), Box<dyn Error>> {
    let mut cmd = Command::new("fastchess");

    cmd.arg("-engine")
        .arg(format!("cmd={}", config.engine))
        .arg("name=plus");
    cmd.arg("option.Hash=64");
    for (name, value) in plus_options {
        cmd.arg(format!("option.{}={}", name, value));
    }

    cmd.arg("-engine")
        .arg(format!("cmd={}", config.engine))
        .arg("name=minus");
    cmd.arg("option.Hash=64");
    for (name, value) in minus_options {
        cmd.arg(format!("option.{}={}", name, value));
    }

    #[rustfmt::skip]
    cmd.args([
        "-each", &format!("tc={}", config.tc),
        "-rounds", &(config.games / 2).to_string(),
        "-repeat",
        "-srand", "1234",
        "-openings", &format!("file={}", config.book), "format=pgn", "order=random",
        "-concurrency", &config.concurrency.to_string(),
        "-recover",
        "-ratinginterval", "0",
    ]);

    let child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped());

    #[cfg(unix)]
    let mut child = child.process_group(0).spawn()?;

    #[cfg(not(unix))]
    let mut child = child.spawn()?;

    loop {
        if !running.load(Ordering::Relaxed) {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }

            child.kill()?;
            child.wait()?;
            return Err("cancelled".into());
        }

        match child.try_wait()? {
            Some(_) => break,
            None => thread::sleep(Duration::from_millis(50)),
        }
    }

    let output = child.wait_with_output()?;

    parse_score(&String::from_utf8_lossy(&output.stdout))
}

fn parse_score(output: &str) -> Result<(isize, isize, isize), Box<dyn Error>> {
    output
        .lines()
        .find(|line| line.starts_with("Results of"))
        .ok_or::<Box<dyn Error>>(
            format!(
                "Could not find results header in fastchess output:\n{}",
                output
            )
            .into(),
        )?;

    let stats_line = output
        .lines()
        .find(|line| line.starts_with("Games:"))
        .ok_or::<Box<dyn Error>>("Could not find stats line in fastchess output.".into())?;

    let mut wins: Option<isize> = None;
    let mut draws: Option<isize> = None;
    let mut losses: Option<isize> = None;

    for part in stats_line.split(',') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("Wins:") {
            wins = Some(v.trim().parse().unwrap());
        } else if let Some(v) = part.strip_prefix("Draws:") {
            draws = Some(v.trim().parse().unwrap());
        } else if let Some(v) = part.strip_prefix("Losses:") {
            losses = Some(v.trim().parse().unwrap());
        }
    }

    let wins = wins.expect("Missing 'Wins' in stats line.");
    let draws = draws.expect("Missing 'Draws' in stats line.");
    let losses = losses.expect("Missing 'Losses' in stats line.");

    Ok((wins, draws, losses))
}

#[test]
fn test_parse_score() {
    let output = r#"
Finished game 997 (ferrischess vs ferrischess_same): 1-0 {White mates}
--------------------------------------------------
Results of ferrischess vs ferrischess_same (1+0.01, 1t, 96MB, 8moves_v3.pgn):
Elo: 7.99 +/- 17.86, nElo: 9.65 +/- 21.53
LOS: 81.01 %, DrawRatio: 36.40 %, PairsRatio: 1.06
Games: 1000, Wins: 377, Losses: 354, Draws: 269, Points: 511.5 (51.15 %)
Ptnml(0-2): [55, 99, 182, 96, 68], WL/DD Ratio: 3.92
--------------------------------------------------
Finished match
Total Time: 00:06:14 (hours:minutes:seconds)
"#;

    let score = parse_score(output).unwrap();
    assert_eq!(score, (377, 269, 354));
}
