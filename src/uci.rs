use std::{
    io::{BufRead, BufReader, Error, Write},
    process::{Command, Stdio},
    str::FromStr,
};

use chessframe::uci::{UciCommand, UciOption};

pub fn get_tunable_params(engine: &str) -> Result<Vec<UciOption>, Error> {
    let mut engine = Command::new(engine)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let mut stdin = engine.stdin.take().unwrap();
    let stdout = engine.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    writeln!(stdin, "uci")?;
    stdin.flush()?;

    let mut line = String::new();
    let mut options = Vec::new();

    loop {
        line.clear();
        let _ = reader.read_line(&mut line)?;

        if line.trim() == "uciok" {
            break;
        }

        if let Ok(UciCommand::Option(option)) = UciCommand::from_str(line.trim())
            && option.name.chars().next().is_some_and(|c| c.is_lowercase())
        {
            options.push(option);
        }
    }

    writeln!(stdin, "quit")?;
    engine.wait()?;

    Ok(options)
}
