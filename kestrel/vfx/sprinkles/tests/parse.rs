//! Effect files parse and migrate without a GPU or an asset server.
use bevy_sprinkles::asset::versions::{self, MigrationError};

#[test]
fn current_version_parses_directly() {
    let result = versions::migrate_str(include_str!("fixtures/valid_particle_system.ron")).unwrap();
    assert!(!result.was_migrated);
    assert_eq!(result.asset.name, "Test Particle System");
    assert_eq!(result.asset.emitters.len(), 1);
    assert_eq!(result.asset.emitters[0].name, "Test Emitter");
    assert_eq!(result.asset.emitters[0].emission.particles_amount, 16);
}

#[test]
fn outdated_version_migrates() {
    let result = versions::migrate_str(include_str!("fixtures/v0_1_particle_system.ron")).unwrap();
    assert!(result.was_migrated);
    assert_eq!(result.asset.name, "gun_shot");
    assert_eq!(result.asset.emitters[0].name, "Sparks");
}

#[test]
fn unknown_version_is_rejected() {
    let error = versions::migrate_str(include_str!("fixtures/unknown_version_particle_system.ron"));
    assert!(matches!(error, Err(MigrationError::UnknownVersion(v)) if v == "67.67"));
}

#[test]
fn a_file_without_a_version_is_rejected() {
    let error = versions::migrate_str(include_str!("fixtures/invalid_particle_system.ron"));
    assert!(matches!(error, Err(MigrationError::Ron(_))));
}

#[test]
fn a_misspelled_field_is_rejected() {
    // Kestrel's fork denies unknown fields, so a typo isn't silently a default.
    let source = include_str!("fixtures/valid_particle_system.ron")
        .replace("particles_amount", "particle_amount");
    assert!(matches!(versions::migrate_str(&source), Err(MigrationError::Ron(_))));
}

#[test]
fn world_velocity_and_even_ring_parse_and_default_off() {
    let source = include_str!("fixtures/valid_particle_system.ron");
    let plain = versions::migrate_str(source).unwrap().asset;
    assert_eq!(plain.emitters[0].velocities.world_velocity.to_array(), [0.0; 3]);
    assert!(!plain.emitters[0].velocities.even_ring);

    // Kestrel's fields (UPSTREAM.md).
    let source = source.replace(
        "emission: (",
        "velocities: (world_velocity: (0.0, 1.5, 0.0), even_ring: true),\n            emission: (",
    );
    let set = versions::migrate_str(&source).unwrap().asset;
    assert_eq!(set.emitters[0].velocities.world_velocity.to_array(), [0.0, 1.5, 0.0]);
    assert!(set.emitters[0].velocities.even_ring);
}
