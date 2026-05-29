use axum::{
    extract::State,
    response::{IntoResponse, Redirect},
    Form,
};
use serde::Deserialize;

use crate::{
    cmd::{self, app, try_run},
    volume_control_available, RunningConf,
};

/// Get current volume percentage using wpctl (PipeWire/WirePlumber) or pactl (PulseAudio/PipeWire compat)
/// Returns None if no volume control tool is available
pub fn get_current_volume() -> Option<u8> {
    // Try wpctl first (PipeWire native)
    if app("wpctl").is_some() {
        if let Some(mut cmd) = app("wpctl") {
            cmd.args(["get-volume", "@DEFAULT_AUDIO_SINK@"]);
            if let Ok(output) = cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Parse "Volume: 0.75" format
                if let Some(vol) = stdout
                    .lines()
                    .find(|l| l.contains("Volume:"))
                    .and_then(|l| {
                        l.split_whitespace()
                            .last()
                            .and_then(|v| v.parse::<f32>().ok())
                    })
                    .map(|v| (v * 100.0) as u8)
                {
                    return Some(vol);
                }
            }
        }
    }

    // Fallback to pactl (PulseAudio/PipeWire compatibility)
    if app("pactl").is_some() {
        if let Some(mut cmd) = app("pactl") {
            cmd.args(["list", "sinks"]);
            if let Ok(output) = cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Find the default sink and extract volume percentage
                for line in stdout.lines() {
                    if line.contains("Volume:") && line.contains('%') {
                        // Parse "Volume: front-left: 65536 / 100% / 0.00 dB,..."
                        if let Some(percent_start) = line.find('/') {
                            let after_slash = &line[percent_start + 1..];
                            if let Some(percent_end) = after_slash.find('%') {
                                if let Ok(vol) = after_slash[..percent_end].trim().parse::<u8>() {
                                    return Some(vol);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None // No volume control available
}

/// Set volume up or down by 5%
fn change_volume(direction: &str) {
    let (wpctl_change, pactl_change) = match direction {
        "up" => ("5%+", "+5%"),
        "down" => ("5%-", "-5%"),
        _ => return,
    };

    // Try wpctl first (PipeWire native)
    if let Some(mut cmd) = app("wpctl") {
        cmd.args(["set-volume", "@DEFAULT_AUDIO_SINK@", wpctl_change]);
        if cmd.status().map(|s| s.success()).unwrap_or(false) {
            return;
        }
    }

    // Fallback to pactl (PulseAudio/PipeWire compatibility)
    if let Some(mut cmd) = app("pactl") {
        cmd.args(["set-sink-volume", "@DEFAULT_SINK@", pactl_change]);
        let _ = cmd.status();
    }
}

#[derive(Deserialize)]
pub struct LimiterQuery {
    enable: Option<String>,
}

pub async fn limiter_handler(
    State(config): State<RunningConf>,
    Form(query): Form<LimiterQuery>,
) -> impl IntoResponse {
    if query.enable.as_deref() == Some("on") {
        config.0.lock().expect("").limiter_on = true;
        try_run(cmd::app("flatpak").map(|mut a| {
            a.args(["run", "com.github.wwmm.easyeffects", "-b", "2"]);
            a
        }));
    } else {
        config.0.lock().expect("").limiter_on = false;
        try_run(cmd::app("flatpak").map(|mut a| {
            a.args(["run", "com.github.wwmm.easyeffects", "-b", "1"]);
            a
        }));
    }
    Redirect::to("/")
}

#[derive(Deserialize)]
pub struct VolumeQuery {
    direction: String,
}

pub async fn volume_handler(Form(query): Form<VolumeQuery>) -> impl IntoResponse {
    // Only change volume if control is available
    if volume_control_available() {
        match query.direction.as_str() {
            "up" => change_volume("up"),
            "down" => change_volume("down"),
            _ => {}
        }
    }
    Redirect::to("/")
}
