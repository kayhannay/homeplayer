use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::Duration;

use gpiocdev::Request;
use gpiocdev::line::{Bias, EdgeDetection};
use tracing::{error, info, warn};

use crate::config::{PowerButtonAction, PowerConfig};

/// Spawns a thread that watches the configured GPIO line and sends `()` on
/// `tx` whenever the power button is pressed. Does nothing if no button action
/// or pin is configured.
pub fn spawn_button_listener(config: &PowerConfig, tx: Sender<()>, ctx: egui::Context) {
    if config.button_action == PowerButtonAction::None {
        return;
    }
    let Some(pin) = config.gpio_pin else {
        warn!("Power button action is set but no gpio_pin is configured");
        return;
    };
    let chip = config.gpio_chip.clone();
    let active_low = config.active_low;

    let spawned = std::thread::Builder::new()
        .name("power-button".into())
        .spawn(move || {
            let mut builder = Request::builder();
            builder
                .on_chip(&chip)
                .with_consumer("homeplayer-power")
                .with_line(pin)
                .as_input()
                .with_debounce_period(Duration::from_millis(50));
            if active_low {
                builder
                    .with_bias(Bias::PullUp)
                    .with_edge_detection(EdgeDetection::FallingEdge);
            } else {
                builder
                    .with_bias(Bias::PullDown)
                    .with_edge_detection(EdgeDetection::RisingEdge);
            }
            let request = match builder.request() {
                Ok(r) => r,
                Err(e) => {
                    error!("Cannot request GPIO line {pin} on {chip}: {e}");
                    return;
                }
            };
            info!("Listening for power button on {chip} line {pin}");
            loop {
                match request.read_edge_event() {
                    Ok(_) => {
                        if tx.send(()).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        error!("Power button listener stopped: {e}");
                        return;
                    }
                }
            }
        });
    if let Err(e) = spawned {
        error!("Failed to spawn power button thread: {e}");
    }
}

/// Runs a shutdown/restart command (program followed by its arguments).
pub fn run(command: &[String]) {
    let Some((program, args)) = command.split_first() else {
        warn!("Power command is empty");
        return;
    };
    info!("Running power command: {command:?}");
    if let Err(e) = Command::new(program).args(args).spawn() {
        error!("Failed to run {program}: {e}");
    }
}
