//! Follows the Mac's output volume. When the volume keys or the Sound menu change it, every
//! streaming AirPlay receiver is scaled by the same ratio, so they move together with the Mac.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;

use tauri::AppHandle;

use crate::controller::Controller;

/// One line from the helper: the Mac's output level (0 to 100) and whether it is muted.
#[derive(Debug, PartialEq)]
pub struct Level {
    pub percent: f64,
    pub muted: bool,
}

/// Reads a line the helper prints: `level <0-100> muted <0|1>`.
pub fn parse_line(line: &str) -> Option<Level> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "level" {
        return None;
    }
    let percent: f64 = parts.next()?.parse().ok()?;
    if parts.next()? != "muted" {
        return None;
    }
    let muted = match parts.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    Some(Level { percent, muted })
}

/// The factor to scale by when the Mac's level moves from `previous` to `next`.
/// None when there is no level to scale from (the Mac was silent).
pub fn ratio(previous: f64, next: f64) -> Option<f64> {
    if previous <= 0.0 {
        None
    } else {
        Some(next / previous)
    }
}

/// A receiver's level after scaling by `ratio`, kept between 0 and 100.
pub fn scaled(level: f64, ratio: f64) -> f64 {
    (level * ratio).clamp(0.0, 100.0)
}

/// Starts the helper that reports the Mac's output level, and scales the receivers as it changes.
/// It runs for the life of the app.
pub fn start(controller: Arc<Controller>, app: AppHandle) -> Result<(), String> {
    let tap = crate::paths::tap_binary().ok_or("the tap binary is missing")?;
    let mut child = Command::new(tap)
        .arg("follow-volume")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not follow the Mac's volume: {error}"))?;
    let stdout = child.stdout.take().ok_or("no output from the volume helper")?;
    // The helper exits when its stdin closes, so both are kept for the life of the app.
    let stdin = child.stdin.take();
    std::thread::spawn(move || {
        let _keep_open = (stdin, child);
        let mut previous: Option<f64> = None;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Some(level) = parse_line(&line) else { continue };
            // While the output on this Mac is muted by mao, its level is not the Mac's volume.
            if level.muted {
                continue;
            }
            let scale = previous.and_then(|before| ratio(before, level.percent));
            controller.mac_volume_changed(&app, level.percent, scale);
            previous = Some(level.percent);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_level_and_mute() {
        assert_eq!(parse_line("level 37.5000 muted 0"), Some(Level { percent: 37.5, muted: false }));
        assert_eq!(parse_line("level 0.0000 muted 1"), Some(Level { percent: 0.0, muted: true }));
    }

    #[test]
    fn ignores_other_lines() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("level x muted 0"), None);
        assert_eq!(parse_line("level 10 muted 2"), None);
        assert_eq!(parse_line("system-output device is now Speakers"), None);
    }

    #[test]
    fn ratio_is_the_change_in_level() {
        assert_eq!(ratio(25.0, 50.0), Some(2.0));
        assert_eq!(ratio(50.0, 25.0), Some(0.5));
        assert_eq!(ratio(0.0, 25.0), None);
    }

    #[test]
    fn scaled_levels_stay_between_zero_and_full() {
        assert_eq!(scaled(40.0, 0.5), 20.0);
        assert_eq!(scaled(80.0, 2.0), 100.0);
        assert_eq!(scaled(10.0, 0.0), 0.0);
    }
}
