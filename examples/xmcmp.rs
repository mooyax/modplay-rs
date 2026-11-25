//! Compares the Rust XM engine against libopenmpt:
//! - playback position (order, row) over time
//! - loudness envelope similarity and level ratio
//! usage: itcmp <file.it> [seconds] [--wav out_prefix]

#[cfg(has_openmpt)]
fn main() {
    use modplay::xm::XmPlayer;
    use modplay::openmpt::OpenMptModule;

    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let secs: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60.0);
    let wav = args.iter().position(|a| a == "--wav").map(|i| args[i + 1].clone());
    let data = std::fs::read(path).unwrap();
    let rate = 44100u32;

    let m = match modplay::xm::format::load(&data) {
        Ok(m) => m,
        Err(e) => {
            println!("RESULT {path} load-error {e}");
            return;
        }
    };
    let nch = m.num_channels;
    let mut ours = XmPlayer::new(m, rate);
    ours.play_from(0);
    let mut mpt = OpenMptModule::load(&data).unwrap();
    if let Some(i) = args.iter().position(|a| a == "--solo") {
        let solo: usize = args[i + 1].parse().unwrap();
        for c in 0..nch {
            if c != solo {
                ours.muted[c] = true;
                assert!(mpt.set_channel_mute(c as i32, true));
            }
        }
    }
    mpt.set_repeat_count(-1);
    // linear interpolation like ours
    unsafe_set_interp(&mut mpt);

    const CHUNK: usize = 256;
    let total = (secs * rate as f64) as usize / CHUNK;
    let mut a = vec![0f32; CHUNK * 2];
    let mut b16 = vec![0i16; CHUNK * 2];
    let (mut same, mut n) = (0usize, 0usize);
    let mut first_diff: Option<(f64, (i32, i32), (i32, i32))> = None;
    let mut env_a = Vec::new();
    let mut env_b = Vec::new();
    let mut wav_a: Vec<f32> = Vec::new();
    let mut wav_b: Vec<f32> = Vec::new();
    let mut prev_mpt_pos = (-1, -1);
    let trace = args.iter().any(|a| a == "--trace");
    let (mut ta, mut tb): (Vec<(i32, i32)>, Vec<(i32, i32)>) = (Vec::new(), Vec::new());
    for i in 0..total {
        ours.render_f32(&mut a);
        let got = mpt.read_stereo(rate, &mut b16);
        if got == 0 || !ours.playing {
            break;
        }
        let pa = (ours.cur_pos as i32, ours.cur_row as i32);
        let pb = (mpt.order(), mpt.row());
        if trace {
            if ta.last() != Some(&pa) { ta.push(pa); }
            if tb.last() != Some(&pb) { tb.push(pb); }
        }
        n += 1;
        // allow a one-chunk skew around row changes
        if pa == pb || pa == prev_mpt_pos {
            same += 1;
        } else if first_diff.is_none() {
            first_diff = Some((i as f64 * CHUNK as f64 / rate as f64, pa, pb));
        }
        prev_mpt_pos = pb;
        let ea: f32 = a.iter().map(|x| x * x).sum::<f32>() / a.len() as f32;
        let eb: f32 = b16.iter().map(|&x| (x as f32 / 32768.0).powi(2)).sum::<f32>() / b16.len() as f32;
        env_a.push(ea.sqrt());
        env_b.push(eb.sqrt());
        if wav.is_some() {
            wav_a.extend_from_slice(&a);
            wav_b.extend(b16.iter().map(|&x| x as f32 / 32768.0));
        }
    }
    let corr = pearson(&env_a, &env_b);
    let ra: f32 = env_a.iter().sum::<f32>();
    let rb: f32 = env_b.iter().sum::<f32>();
    let fd = match first_diff {
        Some((t, pa, pb)) => format!("first_diff@{t:.2}s ours={pa:?} mpt={pb:?}"),
        None => "no_diff".into(),
    };
    println!(
        "RESULT {path} pos_match={:.1}% env_corr={:.3} level_ratio={:.2} chunks={n} {fd}",
        100.0 * same as f64 / n.max(1) as f64,
        corr,
        ra / rb.max(1e-9)
    );
    if trace {
        let f = |v: &[(i32, i32)]| v.iter().take(60).map(|(o, r)| format!("{o}:{r}")).collect::<Vec<_>>().join(" ");
        println!("ours: {}", f(&ta));
        println!("mpt : {}", f(&tb));
    }
    if let Some(prefix) = wav {
        write_wav(&format!("{prefix}_ours.wav"), &wav_a, rate);
        write_wav(&format!("{prefix}_mpt.wav"), &wav_b, rate);
    }
}

#[cfg(has_openmpt)]
fn unsafe_set_interp(m: &mut modplay::openmpt::OpenMptModule) {
    m.set_interpolation(2);
}

#[cfg(has_openmpt)]
fn pearson(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len()) as f32;
    if n < 2.0 {
        return 0.0;
    }
    let ma = a.iter().sum::<f32>() / n;
    let mb = b.iter().sum::<f32>() / n;
    let (mut sab, mut saa, mut sbb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma).powi(2);
        sbb += (y - mb).powi(2);
    }
    sab / (saa.sqrt() * sbb.sqrt()).max(1e-12)
}

#[cfg(has_openmpt)]
fn write_wav(path: &str, s: &[f32], rate: u32) {
    let v: Vec<i16> = s.iter().map(|&x| (x * 32767.0).clamp(-32768.0, 32767.0) as i16).collect();
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    modplay::write_wav(&mut f, &v, rate).unwrap();
}

#[cfg(not(has_openmpt))]
fn main() {
    eprintln!("built without libopenmpt");
}
