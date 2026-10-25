//! Render the native procedural cue bank into browser-ready PCM clips.
use std::{env, fs, io::{self, Write}, path::Path};

#[repr(C)]
struct SynthEvent {
    sample: u64,
    kind: i32,
}

unsafe extern "C" {
    fn kestrel_audio_cue_count() -> i32;
    fn kestrel_audio_synthesize(events: *const SynthEvent, count: u32, output: *mut f32, frames: u32);
}

const SAMPLE_RATE: u32 = 48_000;
const FRAMES: u32 = SAMPLE_RATE * 5 / 4;

fn write_cue(path: &Path, kind: i32) -> io::Result<()> {
    let event = SynthEvent { sample: 0, kind };
    let mut samples = vec![0.0_f32; FRAMES as usize * 2];
    unsafe { kestrel_audio_synthesize(&event, 1, samples.as_mut_ptr(), FRAMES) };

    let data_bytes = FRAMES * 4;
    let mut file = io::BufWriter::new(fs::File::create(path)?);
    file.write_all(b"RIFF")?;
    file.write_all(&(data_bytes + 36).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&2_u16.to_le_bytes())?;
    file.write_all(&SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&(SAMPLE_RATE * 4).to_le_bytes())?;
    file.write_all(&4_u16.to_le_bytes())?;
    file.write_all(&16_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    for sample in samples {
        file.write_all(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    file.flush()
}

fn main() -> io::Result<()> {
    let directory = env::args_os().nth(1).expect("output directory");
    let directory = Path::new(&directory);
    fs::create_dir_all(directory)?;
    let cue_count = unsafe { kestrel_audio_cue_count() };
    assert_eq!(cue_count, 20, "update the declared web cue outputs");
    for kind in 0..cue_count {
        write_cue(&directory.join(format!("cue_{kind}.wav")), kind)?;
    }
    Ok(())
}
