use std::io::Write;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&a[1]).unwrap();
    let mut out = std::fs::File::create(&a[2]).unwrap();
    let mut buf = vec![0f32; 2048];
    let secs: f64 = a[3].parse().unwrap();
    if a[1].ends_with(".xm") {
        let mut p = modplay::xm::player::XmPlayer::new(modplay::xm::format::load(&d).unwrap(), 48000);
        p.play_from(0);
        for _ in 0..(secs * 48000.0 / 1024.0) as usize { p.render_f32(&mut buf); out.write_all(bytemuck(&buf)).unwrap(); }
    } else {
        let mut p = modplay::it::ItPlayer::new(modplay::it::ItModule::load_any(&d).unwrap(), 48000);
        p.play_from(0);
        for _ in 0..(secs * 48000.0 / 1024.0) as usize { p.render_f32(&mut buf); out.write_all(bytemuck(&buf)).unwrap(); }
    }
}
fn bytemuck(b: &[f32]) -> &[u8] { unsafe { std::slice::from_raw_parts(b.as_ptr() as *const u8, b.len() * 4) } }
