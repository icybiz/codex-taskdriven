use super::*;
use pretty_assertions::assert_eq;

#[test]
fn round_trip_and_instruction_replacement_preserve_unrelated_instructions() {
    let mut config = TaskDrivenConfig {
        enabled: true,
        ..Default::default()
    };
    config.task_mut("implementation").unwrap().reasoning_effort = ReasoningEffort::Ultra;
    let serialized = toml::to_string(&config).unwrap();
    let parsed = TaskDrivenConfig::parse(Path::new("test.toml"), &serialized).unwrap();
    assert_eq!(parsed, config);

    let first = enable_instructions(Some("Keep the user's formatting."), &config);
    let second = enable_instructions(Some(&first), &config);
    assert_eq!(first, second);
    assert_eq!(
        strip_instructions(Some(&second)),
        Some("Keep the user's formatting.".to_string())
    );
}

#[test]
fn save_replaces_configuration_and_loads_it_again() {
    let home = tempfile::tempdir().unwrap();
    let config = TaskDrivenConfig {
        enabled: true,
        initial_task: "implementation".to_string(),
        ..Default::default()
    };
    config.save(home.path()).unwrap();
    assert_eq!(TaskDrivenConfig::load(home.path()).unwrap(), config);
}

#[test]
fn validation_rejects_duplicate_and_dangling_routes() {
    let mut duplicate = TaskDrivenConfig::default();
    duplicate.tasks[1].id = duplicate.tasks[0].id.clone();
    assert!(
        duplicate
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );

    let dangling = TaskDrivenConfig {
        initial_task: "missing".to_string(),
        ..Default::default()
    };
    assert!(
        dangling
            .validate()
            .unwrap_err()
            .to_string()
            .contains("initial_task")
    );
}
