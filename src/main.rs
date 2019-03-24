use clap::{App, Arg};
use indicatif::{ProgressBar, ProgressStyle};
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader, Result};
use std::time::Duration;
use uom::si::f64::*;
use uom::si::length::millimeter;
use uom::si::time::second;
use uom::si::velocity::{micrometer_per_second, millimeter_per_second};

#[derive(Debug, Default)]
struct GcodeLineInfo {
    distance: Length,
    time: Time,
}

#[derive(Debug, Default)]
struct GcodeInfo {
    slicer_estimated_duration: Option<Duration>,
    layers: Vec<GcodeLineInfo>,
}

impl GcodeInfo {
    fn layer_change_time(&self) -> Duration {
        let t: Time = Time::new::<second>(9.5) * (self.layers.len() as f64);
        let secs = t.get::<second>();
        Duration::new(secs.floor() as u64, (secs.fract() * 1_000_000.0).floor() as u32)
    }

    fn laser_time(&self) -> Duration {
        let t: Time = self.layers.iter().map(|l| l.time).sum();
        let secs = t.get::<second>();
        Duration::new(secs.floor() as u64, (secs.fract() * 1_000_000.0).floor() as u32)
    }

    fn total_time(&self) -> Duration {
        self.layer_change_time() + self.laser_time()
    }
}

fn parse_file(file: File) -> Result<GcodeInfo> {
    let mut gcode_info: GcodeInfo = Default::default();
    let mut current_x: f64 = 0.0;
    let mut current_y: f64 = 0.0;
    let mut current_feedrate: Velocity = Velocity::new::<millimeter_per_second>(0.0);
    let mut current_layer: Option<usize> = None;

    let progress_bar = ProgressBar::new(file.metadata()?.len());
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{wide_bar}] [{elapsed_precise}] {bytes}/{total_bytes} (ETA: {eta})"),
    );
    progress_bar.set_message("Reading gcode lines");
    let reader = BufReader::new(progress_bar.wrap_read(file));

    for line in reader.lines() {
        let line = line?;
        if line.starts_with(";TIME:") {
            let t = line[6..]
                .parse::<u32>()
                .expect("gcode contains invalid ';TIME:' estimate");
            // Peopoly's own slicer doesn't do time estimates, but still inserts
            // a fake time. We detect this so we don't show a misleading
            // comparison later.
            if t != 6666 {
                gcode_info.slicer_estimated_duration = Some(Duration::from_secs(t as u64));
            }
        } else if line.starts_with(";LAYER:") {
            let cl = line[7..].parse().unwrap();
            current_layer = Some(cl);
            if cl > 0 {
                progress_bar.set_message(&format!("Processed {} layers", cl));
            }
        } else if line.starts_with("G0 ") || line.starts_with("G1 ") {
            let layer_index = match current_layer {
                Some(li) => li,
                None => continue,
            };
            let mut x: Option<f64> = None;
            let mut y: Option<f64> = None;
            let mut f: Option<Velocity> = None;
            for part in line.split_whitespace() {
                match part.split_at(1) {
                    ("F", s) =>
                        // The value here is given in micrometer/minute, and we
                        // need it as micrometer/sec. To do this, we multiply by
                        // 60 to get micrometer/sec (before tagging it with the
                        // unit).
                        f = Some(Velocity::new::<micrometer_per_second>(s.parse::<f64>().unwrap() * 60.0)
                        ),
                    ("X", s) => x = Some(s.parse().unwrap()),
                    ("Y", s) => y = Some(s.parse().unwrap()),
                    _ => (),
                };
            }

            let old_x = current_x;
            let old_y = current_y;
            current_x = x.unwrap_or(current_x);
            current_y = y.unwrap_or(current_y);
            current_feedrate = f.unwrap_or(current_feedrate);
            let delta_x = Length::new::<millimeter>(current_x - old_x);
            let delta_y = Length::new::<millimeter>(current_y - old_y);
            let this_distance: Length = ((delta_x * delta_x) + (delta_y * delta_y)).sqrt();
            let this_time: Time = this_distance / current_feedrate;
            let layer_info = loop {
                match gcode_info.layers.get_mut(layer_index) {
                    None => gcode_info.layers.push(Default::default()),
                    Some(li) => break li,
                }
            };
            layer_info.distance += this_distance;
            layer_info.time += this_time;
        }
    }

    Ok(gcode_info)
}

struct PrettyDuration(Duration);

impl fmt::Display for PrettyDuration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const SECONDS_PER_MINUTE: u64 = 60;
        const MINUTES_PER_HOUR: u64 = 60;
        const SECONDS_PER_HOUR: u64 = SECONDS_PER_MINUTE * MINUTES_PER_HOUR;
        const HOURS_PER_DAY: u64 = 24;
        const SECONDS_PER_DAY: u64 = SECONDS_PER_HOUR * HOURS_PER_DAY;

        let PrettyDuration(d) = *self;
        let total_secs = d.as_secs();
        let (days, rest) = (total_secs / SECONDS_PER_DAY, total_secs % SECONDS_PER_DAY);
        let (hours, rest) = (rest / SECONDS_PER_HOUR, rest % SECONDS_PER_HOUR);
        let (minutes, seconds) = (rest / SECONDS_PER_MINUTE, rest % SECONDS_PER_MINUTE);
        let millis = d.subsec_millis();

        match (days, hours, minutes, seconds) {
            (0, 0, 0, 0) if millis == 0 => write!(f, "0 seconds"),
            (0, 0, 0, 1) if millis == 0 => write!(f, "1 second"),
            (0, 0, 0, s) => write!(f, "{}.{:>04} seconds", s, millis),
            (0, 0, 1, 0) => write!(f, "1 minute"),
            (0, 0, 1, 1) => write!(f, "1 minute and 1 second"),
            (0, 0, 1, s) => write!(f, "1 minute and {} seconds", s),
            (0, 0, m, 0) => write!(f, "{} minutes", m),
            (0, 0, m, 1) => write!(f, "{} minutes 1 second", m),
            (0, 0, m, s) => write!(f, "{} minutes and {} seconds", m, s),
            (0, 1, 0, _) => write!(f, "1 hour"),
            (0, 1, 1, _) => write!(f, "1 hour and 1 minute"),
            (0, 1, m, _) => write!(f, "1 hour and {} minutes", m),
            (0, h, 0, _) => write!(f, "{} hours", h),
            (0, h, 1, _) => write!(f, "{} hours and 1 minute", h),
            (0, h, m, _) => write!(f, "{} hours and {} minutes", h, m),
            (1, 0, 0, _) => write!(f, "1 day"),
            (1, 0, 1, _) => write!(f, "1 day and 1 minute"),
            (1, 0, m, _) => write!(f, "1 day and {} minutes", m),
            (1, 1, 0, _) => write!(f, "1 day and 1 hour"),
            (1, 1, 1, _) => write!(f, "1 day, 1 hour, and 1 minute"),
            (1, 1, m, _) => write!(f, "1 day, 1 hour, and {} minutes", m),
            (1, h, 0, _) => write!(f, "1 day and {} hours", h),
            (1, h, 1, _) => write!(f, "1 day, {} hours, and 1 minute", h),
            (1, h, m, _) => write!(f, "1 day, {} hours, and {} minutes", h, m),
            (d, 0, 0, _) => write!(f, "{} days", d),
            (d, 0, 1, _) => write!(f, "{} days and 1 minute", d),
            (d, 0, m, _) => write!(f, "{} days and {} minutes", d, m),
            (d, 1, 0, _) => write!(f, "{} days and 1 hour", d),
            (d, 1, 1, _) => write!(f, "{} days, 1 hour, and 1 minute", d),
            (d, 1, m, _) => write!(f, "{} days, 1 hour, and {} minutes", d, m),
            (d, h, 0, _) => write!(f, "{} days and {} hours", d, h),
            (d, h, 1, _) => write!(f, "{} days, {} hours, and 1 minute", d, h),
            (d, h, m, _) => write!(f, "{} days, {} hours, and {} minutes", d, h, m),
        }
    }
}

fn main() -> Result<()> {
    let matches = App::new("moai-time")
        .version("0.1")
        .author("Nick Meharry <nick@nickmeharry.com>")
        .about("More accurate time estimation for Peopoly Moai gcode files.")
        .arg(
            Arg::with_name("INPUT")
                .help("Sets the input file to use")
                .required(true)
                .multiple(true)
                .index(1),
        )
        .get_matches();

    for filename in matches.values_of("INPUT").unwrap() {
        let f = File::open(filename)?;
        let parsed = parse_file(f)?;
        let total_time = parsed.total_time();
        println!("For {}:", filename);
        if let Some(est_duration) = parsed.slicer_estimated_duration {
            println!(
                "\tSlicer estimated print time: {}",
                PrettyDuration(est_duration)
            );
        }
        println!(
            "\tEstimated print time: \x1b[32;m{}\x1b[0m",
            PrettyDuration(total_time)
        );
        println!("\t\t       Laser: {}", PrettyDuration(parsed.laser_time()));
        println!(
            "\t\tLayer change: {}",
            PrettyDuration(parsed.layer_change_time())
        );
    }

    Ok(())
}
