//! Scene-authored upright piano with a compact, reactive two-octave keyboard.
//! MIDI is an input adapter; piano notes travel through normal lockstep commands.
use bevy::prelude::*;
use service::WorldScoped;
use voxel_assets::{Scene, ScenePack};

use crate::{control::Options, lockstep::SessionService, scene_view::SceneView, sound::Sound};
use sim::CommandKind;

const FIRST: i32 = 60;
const KEYS: usize = 24;

#[derive(Resource)]
pub struct PianoInput {
    held: [bool; 128],
    connected: bool,
    active: Option<usize>,
    heard: [Option<(u64, u8)>; 128],
}
impl Default for PianoInput {
    fn default() -> Self { Self { held: [false; 128], connected: false, active: None, heard: [None; 128] } }
}

#[derive(Component)]
pub(crate) struct PianoKey { piano: usize, pitch: usize, rest: Vec3, yaw: Quat, scale: Vec3 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Condition { health: [u16; 3], max: [u16; 3] }
impl Default for Condition {
    fn default() -> Self { Self { health: [1; 3], max: [1; 3] } }
}
impl Condition {
    fn segment(pitch: usize) -> usize { pitch * 3 / KEYS }
    fn tone(self, pitch: usize) -> Option<u8> {
        let segment = Self::segment(pitch);
        let (health, max) = (self.health[segment], self.max[segment]);
        if health == 0 || (u32::from(health) * 3 <= u32::from(max) && pitch % 3 == 0) { None }
        else if health == max { Some(0) }
        else if u32::from(health) * 3 <= u32::from(max) { Some(2) }
        else { Some(1) }
    }
}

fn condition(pack: &ScenePack, terrain: Option<&sim::terrain::Terrain>, piano: usize) -> Condition {
    let Some(id) = pack.scene.pianos[piano].damage_asset.as_ref() else { return Condition::default(); };
    let Some(asset) = pack.scene.destructibles.iter().position(|cover| cover.id.as_ref() == Some(id)) else {
        return Condition::default();
    };
    let mut pieces: Vec<_> = pack.destructibles.iter().enumerate()
        .filter(|(_, piece)| usize::from(piece.asset) == asset)
        .collect();
    pieces.sort_by(|a, b| a.1.center[0].total_cmp(&b.1.center[0]));
    let mut state = Condition::default();
    for (segment, (index, piece)) in pieces.into_iter().take(3).enumerate() {
        state.max[segment] = piece.health;
        state.health[segment] = terrain.and_then(|t| t.destructibles.get(index))
            .map_or(piece.health, |state| state.health);
    }
    state
}

fn visible_key(note: u8) -> usize {
    (i32::from(note) - FIRST).rem_euclid(KEYS as i32) as usize
}

fn black(pitch: i32) -> bool { matches!(pitch % 12, 1 | 3 | 6 | 8 | 10) }

fn box_part(world: &mut World, at: Vec3, yaw: Quat, offset: Vec3, size: Vec3,
            mesh: Handle<Mesh>, material: Handle<StandardMaterial>, key: Option<(usize, usize)>) {
    let mut entity = world.spawn((
        Mesh3d(mesh), MeshMaterial3d(material),
        Transform::from_translation(at + yaw * offset).with_rotation(yaw).with_scale(size),
        Visibility::Inherited,
        WorldScoped::default(),
    ));
    if let Some((piano, pitch)) = key {
        entity.insert(PianoKey { piano, pitch, rest: at + yaw * offset, yaw, scale: size });
    }
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
    for (piano_index, piano) in scene.pianos.iter().enumerate() {
        let at = Vec3::from_array(piano.position);
        let yaw = Quat::from_rotation_y(piano.yaw);
        let mut part = |offset: Vec3, size: Vec3, material: Handle<StandardMaterial>| {
            box_part(world, at, yaw, offset, size, cube.clone(), material, None);
        };
        // Damaged pianos get their cabinet from destructible scene geometry;
        // the chassis, pedals and interactive keys remain in this adapter.
        if piano.damage_asset.is_none() {
            part(Vec3::new(0.0, 1.14, -0.46), Vec3::new(2.75, 1.95, 0.32), lacquer.clone());
            part(Vec3::new(0.0, 1.99, -0.30), Vec3::new(2.85, 0.12, 0.63), rim.clone());
            part(Vec3::new(0.0, 1.08, -0.16), Vec3::new(2.78, 0.60, 0.50), lacquer.clone());
            part(Vec3::new(0.0, 0.87, 0.16), Vec3::new(2.78, 0.12, 0.87), rim.clone());
            part(Vec3::new(0.0, 1.48, -0.24), Vec3::new(1.15, 0.07, 0.25), brass.clone());
        } else {
            part(Vec3::new(0.0, 0.17, -0.36), Vec3::new(2.78, 0.22, 0.42), lacquer.clone());
            part(Vec3::new(0.0, 0.87, 0.16), Vec3::new(2.78, 0.12, 0.87), rim.clone());
        }
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
                cube.clone(), ivory.clone(), Some((piano_index, pitch)));
            white_before += 1;
        }
        let mut white_before = 0;
        for pitch in 0..KEYS {
            if !black(pitch as i32) { white_before += 1; continue; }
            let x = -1.21 + white_before as f32 * white_width;
            let offset = Vec3::new(x, 1.04, 0.08);
            box_part(world, at, yaw, offset, Vec3::new(white_width * 0.58, 0.14, 0.30),
                cube.clone(), ebony.clone(), Some((piano_index, pitch)));
        }
    }
}

pub fn release(world: &mut World) {
    let sounding: Vec<u8> = world.resource::<PianoInput>().heard.iter().enumerate()
        .filter_map(|(note, heard)| heard.is_some().then_some(note as u8)).collect();
    for note in sounding { world.resource::<Sound>().note(note, 0); }
    let mut input = world.resource_mut::<PianoInput>();
    input.held.fill(false);
    input.heard.fill(None);
    input.active = None;
}

fn nearby(scene: &Scene, player: Vec3) -> Option<usize> {
    scene.pianos.iter().enumerate().filter_map(|(index, piano)| {
        let delta = Vec3::from_array(piano.position) - player;
        let distance = Vec2::new(delta.x, delta.z).length();
        (distance < sim::settlement::piano::REACH.to_f32()).then_some((index, distance))
    }).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(index, _)| index)
}

fn input_note(input: &mut PianoInput, note: u8, velocity: u8, mut send: impl FnMut(u16, u8, u8)) {
    let index = usize::from(note);
    if index >= input.held.len() { return; }
    if velocity == 0 {
        if input.held[index] {
            if let Some(piano) = input.active { send(piano as u16, note, 0); }
        }
        input.held[index] = false;
    } else if let Some(piano) = input.active {
        input.held[index] = true;
        send(piano as u16, note, velocity);
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
    scene: Res<SceneView>, options: Res<Options>, mut session: ResMut<SessionService>, sound: Res<Sound>,
    mut input: ResMut<PianoInput>, mut poll: Local<MidiPoll>,
    mut keys: Query<(&PianoKey, &mut Transform, &mut Visibility)>,
) {
    let agent = session.player();
    if scene.scene().pianos.is_empty() {
        for n in 0..128 {
            if input.held[n] { input_note(&mut input, n as u8, 0, |piano, pitch, velocity| {
                if let Some(agent) = agent { session.command(CommandKind::PlayPiano { agent, piano, pitch, velocity }); }
            }); }
            if input.heard[n].take().is_some() { sound.note(n as u8, 0); }
        }
        input.active = None;
        return;
    }
    let active = session.player_view_position().and_then(|at| nearby(scene.scene(), at));
    if active != input.active {
        for n in 0..128 {
            if input.held[n] { input_note(&mut input, n as u8, 0, |piano, pitch, velocity| {
                if let Some(agent) = agent { session.command(CommandKind::PlayPiano { agent, piano, pitch, velocity }); }
            }); }
        }
        input.active = active;
    }
    let conditions: Vec<_> = (0..scene.scene().pianos.len())
        .map(|index| condition(&scene.pack, session.world().map(|world| world.terrain()), index))
        .collect();
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
                    0x80 | 0x90 => input_note(&mut input, note,
                        if status & 0xf0 == 0x80 { 0 } else { velocity }, |piano, pitch, velocity| {
                            if let Some(agent) = agent { session.command(CommandKind::PlayPiano { agent, piano, pitch, velocity }); }
                        }),
                    // All notes off / all sound off.
                    0xb0 if note == 120 || note == 123 => {
                        for n in 0..128 { input_note(&mut input, n, 0, |piano, pitch, velocity| {
                            if let Some(agent) = agent { session.command(CommandKind::PlayPiano { agent, piano, pitch, velocity }); }
                        }); }
                    }
                    0xf0 if status == 0xff => {
                        for n in 0..128 { input_note(&mut input, n, 0, |piano, pitch, velocity| {
                            if let Some(agent) = agent { session.command(CommandKind::PlayPiano { agent, piano, pitch, velocity }); }
                        }); }
                    }
                    _ => {}
                }
            });
            input.connected = midi.connected();
        }
    }
    #[cfg(not(target_os = "linux"))]
    { let _ = (&options, &mut poll); }
    if active.is_none() || !input.connected {
        for n in 0..128 {
            if input.held[n] { input_note(&mut input, n as u8, 0, |piano, pitch, velocity| {
                if let Some(agent) = agent { session.command(CommandKind::PlayPiano { agent, piano, pitch, velocity }); }
            }); }
        }
    }
    let mut pressed = vec![[false; KEYS]; scene.scene().pianos.len()];
    let mut wanted: [Option<(u64, u8, u8)>; 128] = [None; 128];
    if let Some(settlement) = session.settlement() {
        for note in &settlement.piano_notes {
            let piano = usize::from(note.piano);
            if piano >= pressed.len() || usize::from(note.pitch) >= 128 { continue; }
            pressed[piano][visible_key(note.pitch)] = true;
            if wanted[usize::from(note.pitch)].is_none_or(|old| note.serial > old.0) {
                if let Some(damage) = conditions[piano].tone(visible_key(note.pitch)) {
                    wanted[usize::from(note.pitch)] = Some((note.serial, note.velocity, damage));
                }
            }
        }
    }
    for (pitch, want) in wanted.into_iter().enumerate() {
        let current = want.map(|(serial, _, damage)| (serial, damage));
        if current != input.heard[pitch] {
            if input.heard[pitch].is_some() { sound.note(pitch as u8, 0); }
            if let Some((_, velocity, damage)) = want {
                sound.note_damaged(pitch as u8, velocity, damage);
            }
            input.heard[pitch] = current;
        }
    }
    for (key, mut transform, mut visibility) in &mut keys {
        let condition = conditions[key.piano];
        let segment = Condition::segment(key.pitch);
        let health = condition.health[segment];
        let damaged = health < condition.max[segment];
        *visibility = if health == 0 { Visibility::Hidden } else { Visibility::Inherited };
        let played = pressed[key.piano][key.pitch];
        transform.translation = key.rest - Vec3::Y * if played { 0.035 } else { 0.0 };
        transform.rotation = key.yaw * Quat::from_rotation_x(if damaged { 0.12 } else { 0.0 });
        transform.scale = key.scale * if damaged { Vec3::new(1.0, 0.88, 0.94) } else { Vec3::ONE };
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
        assert_eq!(nearby(&scene, Vec3::new(0.0, 0.0, -2.4)), Some(0));
        assert_eq!(nearby(&scene, Vec3::new(0.0, 0.0, 1.0)), None);
        scene.pianos.clear();
        assert_eq!(nearby(&scene, Vec3::new(0.0, 0.0, -4.0)), None);
    }
    #[test]
    fn midi_press_requires_proximity_and_release_always_clears() {
        let mut input = PianoInput::default();
        let mut commands = Vec::new();
        input_note(&mut input, 61, 100, |piano, pitch, velocity| commands.push((piano, pitch, velocity)));
        assert!(!input.held[61]);
        input.active = Some(0);
        input_note(&mut input, 61, 100, |piano, pitch, velocity| commands.push((piano, pitch, velocity)));
        assert!(input.held[61]);
        input_note(&mut input, 61, 0, |piano, pitch, velocity| commands.push((piano, pitch, velocity)));
        assert!(!input.held[61]);
        assert_eq!(commands, [(0, 61, 100), (0, 61, 0)]);
    }
    #[test]
    fn cabinet_damage_detunes_some_notes_and_mutes_broken_sections() {
        let mut condition = Condition { health: [3; 3], max: [3; 3] };
        assert_eq!(condition.tone(0), Some(0));
        assert_eq!(condition.tone(12), Some(0));
        condition.health[1] = 2;
        assert_eq!(condition.tone(10), Some(1));
        condition.health[1] = 1;
        assert_eq!(condition.tone(9), None);
        assert_eq!(condition.tone(10), Some(2));
        condition.health[1] = 0;
        assert_eq!(condition.tone(10), None);
        assert_eq!(condition.tone(0), Some(0));
    }
    #[test]
    fn stage_piano_tracks_the_same_damage_as_cover_and_its_keys() {
        use fx::{Fx, FxVec2};
        use sim::terrain::{Destructible, Disc, Terrain};

        let library = crate::assets::SceneLibrary::new();
        let pack = &library.scenes[library.index("laser-pit").unwrap()];
        let mut terrain = Terrain::flat(FxVec2::ZERO, Fx::from_int(20));
        for piece in &pack.destructibles {
            terrain.destructibles.push(Destructible {
                asset: piece.asset,
                disc: Disc { center: FxVec2::ZERO, radius: Fx::ONE },
                health: piece.health,
                max_health: piece.health,
                repairable: piece.repairable,
                repair_progress: 0,
            });
        }
        let piano_asset = pack.scene.destructibles.iter()
            .position(|asset| asset.id.as_deref() == Some("stage-piano")).unwrap();
        let mut pieces: Vec<_> = pack.destructibles.iter().enumerate()
            .filter(|(_, piece)| usize::from(piece.asset) == piano_asset).collect();
        pieces.sort_by(|a, b| a.1.center[0].total_cmp(&b.1.center[0]));
        assert_eq!(pieces.len(), 3);
        assert_eq!(condition(pack, Some(&terrain), 0).tone(10), Some(0));
        assert!(terrain.damage_cover(pieces[1].0, 2));
        assert_eq!(condition(pack, Some(&terrain), 0).tone(9), None);
        assert_eq!(condition(pack, Some(&terrain), 0).tone(10), Some(2));
        assert!(terrain.damage_cover(pieces[1].0, 1));
        assert_eq!(condition(pack, Some(&terrain), 0).tone(10), None);
        assert_eq!(condition(pack, Some(&terrain), 0).tone(0), Some(0));
        assert!(pieces.iter().all(|(_, piece)| piece.repairable && !piece.base_meshes.is_empty()));
        for _ in 0..3 * content::TICKS_PER_SECOND {
            terrain.repair_cover(pieces[1].0, true);
        }
        assert_eq!(condition(pack, Some(&terrain), 0).tone(10), Some(0));
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
