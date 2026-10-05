//! Compile with rustc; no board, Cargo dependencies or USB device required.
#[path = "../src/touch_strip/gesture.rs"]
mod gesture;

use gesture::{Config, Gesture, Recognizer};
use std::io::Read;

const DEMO: &str = "\
0 000000
100 000001
180 000011
260 000010
340 000110
420 000100
500 001000
580 010000
660 100000
740 000000
1000 100000
1080 010000
1160 001000
1240 000100
1320 000010
1400 000001
1480 000000
1800 000100
1880 000000
1980 000100
2060 000000
";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = match std::env::args().nth(1).as_deref() {
        None => DEMO.to_owned(),
        Some("-") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        }
        Some(path) => std::fs::read_to_string(path)?,
    };
    let mut changes = Vec::new();
    for (line_no, line) in source.lines().enumerate() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 2 || fields[1].len() != 6 {
            return Err(format!(
                "line {}: expected milliseconds and six binary bits",
                line_no + 1
            )
            .into());
        }
        let time: u64 = fields[0].parse()?;
        let mask = u8::from_str_radix(fields[1], 2)?;
        if time % 5 != 0 || changes.last().is_some_and(|&(last, _)| time <= last) {
            return Err("timestamps must increase and be multiples of 5 ms".into());
        }
        changes.push((time, mask));
    }
    if changes.first().map(|c| c.0) != Some(0) {
        return Err("the first sample must have timestamp 0".into());
    }
    let end = changes
        .last()
        .unwrap()
        .0
        .checked_add(500)
        .ok_or("timestamp overflow")?;
    let mut recognizer = Recognizer::new(Config::default());
    let mut index = 0;
    for time in (0..=end).step_by(5) {
        while index + 1 < changes.len() && changes[index + 1].0 <= time {
            index += 1;
        }
        if let Some(gesture) = recognizer.sample(time, changes[index].1) {
            let default_key = match gesture {
                Gesture::SwipeUp => "AudioVolUp",
                Gesture::SwipeDown => "AudioVolDown",
                Gesture::DoubleTap => "MediaPlayPause",
            };
            println!(
                "{time:5} ms  {gesture:?} -> {:?} {default_key}",
                gesture.key_position()
            );
        }
    }
    Ok(())
}
