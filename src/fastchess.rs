use std::{collections::HashMap, io::Error, process::Command};

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
) -> Result<(isize, isize), Error> {
    let mut cmd = Command::new("fastchess");

    cmd.arg("-engine")
        .arg(format!("cmd={}", config.engine))
        .arg("name=plus");
    cmd.arg("option.Hash=16");
    for (name, value) in plus_options {
        cmd.arg(format!("option.{}={}", name, value));
    }

    cmd.arg("-engine")
        .arg(format!("cmd={}", config.engine))
        .arg("name=minus");
    cmd.arg("option.Hash=16");
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

    let output = cmd.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    parse_score(&stdout)
}

fn parse_score(output: &str) -> Result<(isize, isize), Error> {
    output
        .lines()
        .find(|line| line.starts_with("Results of"))
        .expect(&format!(
            "Could not find results header in fastchess output:\n{}",
            output
        ));

    let stats_line = output
        .lines()
        .find(|line| line.starts_with("Games:"))
        .expect("Could not find stats line in fastchess output.");

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
    let _draws = draws.expect("Missing 'Draws' in stats line.");
    let losses = losses.expect("Missing 'Losses' in stats line.");

    Ok((wins, losses))
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
    assert_eq!(score, (377, 354));
}
