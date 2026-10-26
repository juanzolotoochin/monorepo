//! Scene-authored upright piano with a compact, reactive two-octave keyboard.
//! MIDI input is local presentation: it never changes lockstep game state.
use bevy::prelude::*;
use service::WorldScoped;
use voxel_assets::Scene;

use crate::{control::Options, lockstep::SessionService, scene_view::SceneView, sound::Sound};

const FIRST: i32 = 60;
const KEYS: usize = 24;
const REACH: f32 = 2.8;

#[derive(Resource)]
pub struct PianoInput {
    held: [bool; 128],
    connected: bool,
}
impl Default for PianoInput {
    fn default() -> Self { Self { held: [false; 128], connected: false } }
}

#[derive(Component)]
pub(crate) struct PianoKey { pitch: usize, rest: Vec3 }

fn visible_key(note: u8) -> usize {
    (i32::from(note) - FIRST).rem_euclid(KEYS as i32) as usize
}

fn black(pitch: i32) -> bool { matches!(pitch % 12, 1 | 3 | 6 | 8 | 10) }

fn box_part(world: &mut World, at: Vec3, yaw: Quat, offset: Vec3, size: Vec3,
            mesh: Handle<Mesh>, material: Handle<StandardMaterial>, key: Option<usize>) {
    let mut entity = world.spawn((
        Mesh3d(mesh), MeshMaterial3d(material),
        Transform::from_translation(at + yaw * offset).with_rotation(yaw).with_scale(size),
        WorldScoped::default(),
    ));
    if let Some(pitch) = key { entity.insert(PianoKey { pitch, rest: at + yaw * offset }); }
}

pub fn spawn(world: &mut World, scene: &Scene) {
    if scene.pianos.is_empty() { return; }
    let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(1.0, 1.0, 1.0));
    let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
    let lacquer = mats.add(Color::srgb(0.10, 0.075, 0.085));
    let rim = mats.add(Color::srgb(0.52, 0.31, 0.13));
    let ivory = mats.add(Color::srgb(0.88, 0.84, 0.71));
    let ebony = mats.add(Color::srgb(0.055, 0.065, 0.085));
    let brass = mats.add(Color::srgb(0.73, 0.54, 0.20));
    let felt = mats.add(Color::srgb(0.34, 0.055, 0.095));
    for piano in &scene.pianos {
        let at = Vec3::from_array(piano.position);
        let yaw = Quat::from_rotation_y(piano.yaw);
        let mut part = |offset: Vec3, size: Vec3, material: Handle<StandardMaterial>| {
            box_part(world, at, yaw, offset, size, cube.clone(), material, None);
        };
        // Upright cabinet, full-width music desk, two front legs and pedals.
        part(Vec3::new(0.0, 1.14, -0.46), Vec3::new(2.75, 1.95, 0.32), lacquer.clone());
        part(Vec3::new(0.0, 1.99, -0.30), Vec3::new(2.85, 0.12, 0.63), rim.clone());
        part(Vec3::new(0.0, 1.08, -0.16), Vec3::new(2.78, 0.60, 0.50), lacquer.clone());
        part(Vec3::new(0.0, 0.87, 0.16), Vec3::new(2.78, 0.12, 0.87), rim.clone());
        part(Vec3::new(0.0, 1.48, -0.24), Vec3::new(1.15, 0.07, 0.25), brass.clone());
        for side in [-1.0_f32, 1.0] {
            part(Vec3::new(side * 1.20, 0.38, 0.15), Vec3::new(0.20, 0.76, 0.26), lacquer.clone());
        }
        for x in [-0.18_f32, 0.0, 0.18] {
            part(Vec3::new(x, 0.11, 0.30), Vec3::new(0.09, 0.04, 0.38), brass.clone());
        }
        part(Vec3::new(0.0, 0.91, -0.01), Vec3::new(2.45, 0.02, 0.08), felt.clone());
        drop(part);
        let white_width = 2.42 / 14.0;
        let mut white_before = 0;
        for pitch in 0..KEYS {
            if black(pitch as i32) { continue; }
            let x = -1.21 + (white_before as f32 + 0.5) * white_width;
            let offset = Vec3::new(x, 0.96, 0.22);
            box_part(world, at, yaw, offset, Vec3::new(white_width - 0.009, 0.09, 0.53),
                cube.clone(), ivory.clone(), Some(pitch));
            white_before += 1;
        }
        let mut white_before = 0;
        for pitch in 0..KEYS {
            if !black(pitch as i32) { white_before += 1; continue; }
            let x = -1.21 + white_before as f32 * white_width;
            let offset = Vec3::new(x, 1.04, 0.08);
            box_part(world, at, yaw, offset, Vec3::new(white_width * 0.58, 0.14, 0.30),
                cube.clone(), ebony.clone(), Some(pitch));
        }
    }
}

pub fn release(world: &mut World) {
    let held: Vec<u8> = world.resource::<PianoInput>().held.iter().enumerate()
        .filter_map(|(note, held)| held.then_some(note as u8)).collect();
    for note in held { world.resource::<Sound>().note(note, 0); }
    world.resource_mut::<PianoInput>().held.fill(false);
}

fn nearby(scene: &Scene, player: Vec3) -> bool {
    scene.pianos.iter().any(|piano| {
        let delta = Vec3::from_array(piano.position) - player;
        Vec2::new(delta.x, delta.z).length() < REACH
    })
}

fn input_note(input: &mut PianoInput, sound: &Sound, note: u8, velocity: u8, can_play: bool) {
    let index = usize::from(note);
    if velocity == 0 {
        if input.held[index] { sound.note(note, 0); }
        input.held[index] = false;
    } else if can_play {
        input.held[index] = true;
        sound.note(note, velocity);
    }
}

#[cfg(target_os = "linux")]
mod midi {
    use std::ffi::{c_char, c_int, c_void};
    pub(super) fn accepts(source: &mut Option<(i32, i32)>, address: (i32, i32), message: [u8; 3]) -> bool {
        if source.is_none() && message[0] & 0xf0 == 0x90 && message[2] > 0 {
            *source = Some(address);
        }
        *source == Some(address)
    }
    unsafe extern "C" {
        fn mk_open(seq: *mut *mut c_void) -> c_int;
        fn mk_close(seq: *mut c_void);
        fn mk_port(seq: *mut c_void, index: c_int, client: *mut c_int, port: *mut c_int,
                   name: *mut c_char, size: usize) -> c_int;
        fn mk_connect(seq: *mut c_void, client: c_int, port: c_int) -> c_int;
        fn mk_event(seq: *mut c_void, message: *mut u8, client: *mut c_int, port: *mut c_int,
                    event_type: *mut c_int, raw: *mut *const u8, raw_size: *mut usize) -> c_int;
    }
    pub struct Midi {
        seq: *mut c_void,
        ports: Vec<(i32, i32)>,
        source: Option<(i32, i32)>,
    }
    // The Bevy system owns this handle exclusively; a scheduler may move the
    // system between threads, but never accesses the handle concurrently.
    unsafe impl Send for Midi {}
    impl Midi {
        pub fn open() -> Option<Self> {
            let mut seq = std::ptr::null_mut();
            (unsafe { mk_open(&mut seq) } >= 0).then_some(Self { seq, ports: Vec::new(), source: None })
        }
        pub fn connect(&mut self) {
            // The first enumerated port is often MIDI Through, not the user's
            // keyboard. Subscribe to every readable port and select the first
            // one that actually sends a note, as midi_keys does.
            for index in 0..128 {
                let (mut client, mut port) = (0, 0);
                let mut name = [0 as c_char; 256];
                if unsafe { mk_port(self.seq, index, &mut client, &mut port, name.as_mut_ptr(), name.len()) } != 1 {
                    break;
                }
                let address = (client, port);
                if !self.ports.contains(&address)
                    && unsafe { mk_connect(self.seq, client, port) } >= 0 {
                    self.ports.push(address);
                }
            }
        }
        pub fn connected(&self) -> bool { self.source.is_some() }
        pub fn drain(&mut self, mut event: impl FnMut(u8, u8, u8)) {
            for _ in 0..256 {
                let mut message = [0; 3];
                let (mut client, mut port, mut kind) = (0, 0, 0);
                let mut raw = std::ptr::null();
                let mut size = 0;
                let result = unsafe { mk_event(self.seq, message.as_mut_ptr(), &mut client,
                    &mut port, &mut kind, &mut raw, &mut size) };
                if result <= 0 { if result < 0 { self.source = None; } break; }
                let address = (client, port);
                if result == 3 {
                    self.ports.retain(|port| *port != address);
                    if self.source == Some(address) { self.source = None; }
                }
                if result == 1 && self.ports.contains(&address) {
                    if accepts(&mut self.source, address, message) {
                        event(message[0], message[1], message[2]);
                    }
                }
            }
        }
    }
    impl Drop for Midi { fn drop(&mut self) { unsafe { mk_close(self.seq); } } }
}

#[derive(Default)]
pub struct MidiPoll {
    frame: u32,
    #[cfg(target_os = "linux")]
    midi: Option<midi::Midi>,
}

pub fn update(
    scene: Res<SceneView>, options: Res<Options>, session: Res<SessionService>, sound: Res<Sound>,
    mut input: ResMut<PianoInput>, mut poll: Local<MidiPoll>,
    mut keys: Query<(&PianoKey, &mut Transform)>,
) {
    if scene.scene().pianos.is_empty() {
        for n in 0..128 {
            if input.held[n] { input_note(&mut input, &sound, n as u8, 0, false); }
        }
        return;
    }
    let can_play = session.player_view_position().is_some_and(|at| nearby(scene.scene(), at));
    #[cfg(target_os = "linux")]
    if !options.simulation && options.frames_dir.is_none() {
        poll.frame = poll.frame.wrapping_add(1);
        if poll.frame % 120 == 1 {
            if poll.midi.is_none() { poll.midi = midi::Midi::open(); }
            if let Some(midi) = &mut poll.midi { midi.connect(); }
        }
        if let Some(midi) = &mut poll.midi {
            midi.drain(|status, note, velocity| {
                match status & 0xf0 {
                    0x80 => input_note(&mut input, &sound, note, 0, can_play),
                    0x90 => input_note(&mut input, &sound, note, velocity, can_play),
                    // All notes off / all sound off.
                    0xb0 if note == 120 || note == 123 => {
                        for n in 0..128 { input_note(&mut input, &sound, n, 0, can_play); }
                    }
                    0xf0 if status == 0xff => {
                        for n in 0..128 { input_note(&mut input, &sound, n, 0, can_play); }
                    }
                    _ => {}
                }
            });
            input.connected = midi.connected();
        }
    }
    #[cfg(not(target_os = "linux"))]
    { let _ = (&options, &mut poll); }
    if !can_play || !input.connected {
        for n in 0..128 {
            if input.held[n] { input_note(&mut input, &sound, n as u8, 0, false); }
        }
    }
    let mut pressed = [false; KEYS];
    for (note, held) in input.held.iter().enumerate() {
        if *held { pressed[visible_key(note as u8)] = true; }
    }
    for (key, mut transform) in &mut keys {
        transform.translation = key.rest - Vec3::Y * if pressed[key.pitch] { 0.035 } else { 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notes_wrap_into_two_visible_octaves_without_losing_pitch_class() {
        for note in 0..128_u8 {
            assert_eq!(visible_key(note) % 12, (i32::from(note) - FIRST).rem_euclid(12) as usize);
        }
        assert_eq!(visible_key(60), 0);
        assert_eq!(visible_key(84), 0);
        assert_eq!(visible_key(72), 12);
    }
    #[test]
    fn piano_plays_only_within_reach() {
        let mut scene = crate::assets::SceneLibrary::new().scenes[12].scene.clone();
        assert_eq!(scene.pianos.len(), 1);
        assert!(nearby(&scene, Vec3::new(0.0, 0.0, -2.4)));
        assert!(!nearby(&scene, Vec3::new(0.0, 0.0, 1.0)));
        scene.pianos.clear();
        assert!(!nearby(&scene, Vec3::new(0.0, 0.0, -4.0)));
    }
    #[test]
    fn midi_press_requires_proximity_and_release_always_clears() {
        let mut input = PianoInput::default();
        let sound = Sound::silent();
        input_note(&mut input, &sound, 61, 100, false);
        assert!(!input.held[61]);
        input_note(&mut input, &sound, 61, 100, true);
        assert!(input.held[61]);
        input_note(&mut input, &sound, 61, 0, false);
        assert!(!input.held[61]);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn midi_selects_first_played_port_instead_of_first_listed_port() {
        let through = (14, 0);
        let keyboard = (32, 0);
        let mut selected = None;
        assert!(!midi::accepts(&mut selected, through, [0xb0, 64, 127]));
        assert!(midi::accepts(&mut selected, keyboard, [0x90, 60, 100]));
        assert_eq!(selected, Some(keyboard));
        assert!(!midi::accepts(&mut selected, through, [0x90, 64, 100]));
        assert!(midi::accepts(&mut selected, keyboard, [0x80, 60, 0]));
    }
}
