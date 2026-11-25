//! pt2wav - render a ProTracker .MOD to .WAV using the ProTracker engine (port of pt2-clone).

use modplay::consts::{MAX_AUDIO_FREQUENCY, MIN_AUDIO_FREQUENCY};
use modplay::{render_mod, write_wav, AmigaModel, Engine, EngineConfig, Mod2WavOptions, Module, TimingMode};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
usage: pt2wav <input.mod> [output.wav] [options]

options:
  --rate <hz>        output rate, 44100..192000 (default 44100)
  --loops <n>        extra song loops, 0..50 (default 0)
  --fadeout <sec>    fade out the last <sec> seconds (1..60)
  --stereo <0-100>   stereo separation in percent (default 20, 100 = hard Amiga panning)
  --model <a500|a1200>  Amiga filter model (default a1200)
  --vblank           use vblank timing instead of CIA
  --e8x              enable E8x (Karplus-Strong) effect
  --128k             allow samples up to 131070 bytes (default 65534)
  --max-seconds <s>  stop rendering after <s> seconds, 0 = unlimited (default 1800)
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_num<T: std::str::FromStr>(args: &mut impl Iterator<Item = String>, name: &str) -> Result<T, String> {
    let v = args.next().ok_or_else(|| format!("{name} needs a value"))?;
    v.parse().map_err(|_| format!("bad value for {name}: {v}"))
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut cfg = EngineConfig::default();
    let mut opt = Mod2WavOptions::default();
    let mut max_seconds: u64 = 1800;

    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            "--rate" => {
                let r: u32 = parse_num(&mut args, "--rate")?;
                opt.output_rate = r.clamp(MIN_AUDIO_FREQUENCY, MAX_AUDIO_FREQUENCY);
            }
            "--loops" => opt.num_loops = parse_num::<i8>(&mut args, "--loops")?.clamp(0, 50),
            "--fadeout" => {
                opt.fadeout = true;
                opt.fadeout_seconds = parse_num::<u32>(&mut args, "--fadeout")?.clamp(1, 60);
            }
            "--stereo" => cfg.stereo_separation = parse_num::<u8>(&mut args, "--stereo")?.min(100),
            "--model" => {
                let m: String = parse_num(&mut args, "--model")?;
                cfg.amiga_model = match m.to_ascii_lowercase().as_str() {
                    "a500" => AmigaModel::A500,
                    "a1200" => AmigaModel::A1200,
                    _ => return Err(format!("unknown model: {m}")),
                };
            }
            "--vblank" => cfg.timing_mode = TimingMode::Vblank,
            "--e8x" => cfg.enable_e8x = true,
            "--128k" => cfg.max_sample_length = 131070,
            "--max-seconds" => max_seconds = parse_num(&mut args, "--max-seconds")?,
            s if s.starts_with("--") => return Err(format!("unknown option: {s}\n\n{USAGE}")),
            _ => {
                if input.is_none() {
                    input = Some(PathBuf::from(a));
                } else if output.is_none() {
                    output = Some(PathBuf::from(a));
                } else {
                    return Err(format!("too many arguments\n\n{USAGE}"));
                }
            }
        }
    }

    let input = input.ok_or_else(|| USAGE.to_string())?;
    let output = output.unwrap_or_else(|| input.with_extension("wav"));

    let data = std::fs::read(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    let song = Module::load(&data, cfg.max_sample_length).map_err(|e| format!("{}: {e}", input.display()))?;
    eprintln!("loaded \"{}\" ({} orders)", song.name_str().trim_end(), song.song_length);

    cfg.output_rate = opt.output_rate;
    opt.max_frames = (max_seconds * opt.output_rate as u64) as usize;
    let mut engine = Engine::new(song, cfg);

    let samples = render_mod(&mut engine, &opt, |_| {});
    let secs = samples.len() as f64 / 2.0 / opt.output_rate as f64;

    let f = std::fs::File::create(&output).map_err(|e| format!("{}: {e}", output.display()))?;
    let mut w = std::io::BufWriter::new(f);
    write_wav(&mut w, &samples, opt.output_rate).map_err(|e| e.to_string())?;
    eprintln!("wrote {} ({:.0}:{:05.2})", output.display(), (secs / 60.0).floor(), secs % 60.0);
    Ok(())
}
