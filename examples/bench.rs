//! render N seconds with the Rust engines at 48 kHz in 1024-frame blocks; time + voice stats
use std::time::Instant;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(600.0);
    let data = std::fs::read(&args[1]).unwrap();
    let mut buf = vec![0f32; 2048];
    let blocks = (secs * 48000.0 / 1024.0) as usize;
    let name = args[1].rsplit('/').next().unwrap();
    #[cfg(has_openmpt)]
    if args.get(3).map(|s| s == "mpt").unwrap_or(false) {
        let mut m = modplay::openmpt::OpenMptModule::load(&data).unwrap();
        m.set_repeat_count(-1);
        let mut b16 = vec![0i16; 2048];
        let t = Instant::now();
        for _ in 0..blocks { m.read_stereo(48000, &mut b16); }
        println!("{name:28} {:.3}s (libopenmpt)", t.elapsed().as_secs_f64());
        return;
    }
    if args[1].ends_with(".xm") {
        let m = modplay::xm::format::load(&data).unwrap();
        let mut p = modplay::xm::player::XmPlayer::new(m, 48000);
        p.play_from(0);
        let t = Instant::now();
        for _ in 0..blocks { p.render_f32(&mut buf); }
        println!("{name:28} {:.3}s", t.elapsed().as_secs_f64());
    } else {
        let m = modplay::it::ItModule::load_any(&data).unwrap();
        let mut p = modplay::it::ItPlayer::new(m, 48000);
        p.play_from(0);
        let (mut act, mut silent, mut n) = (0usize, 0usize, 0usize);
        let mut mix_t = 0.0;
        let t = Instant::now();
        for _ in 0..blocks {
            let t1 = Instant::now();
            p.render_f32(&mut buf);
            mix_t += t1.elapsed().as_secs_f64();
            for c in &p.chn { if c.active { act += 1; if c.target_l == 0.0 && c.target_r == 0.0 && c.gain_l == 0.0 && c.gain_r == 0.0 { silent += 1; } } }
            n += 1;
        }
        println!("{name:28} {:.3}s  avg_active={:.1} avg_silent_active={:.1}", t.elapsed().as_secs_f64(), act as f64 / n as f64, silent as f64 / n as f64);
        let _ = mix_t;
    }
}
