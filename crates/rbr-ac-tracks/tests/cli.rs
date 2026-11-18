//! Check the public command routes without requiring game data.
use std::process::Command;

#[test]
fn each_command_has_help() {
    for command in ["ac", "import", "prepare", "probe"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rbr-ac-tracks")).args([command, "--help"]).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("rbr-ac-tracks {command}")));
    }
}

#[test]
fn missing_inputs_and_unknown_commands_fail() {
    for args in [vec!["not-a-converter"], vec!["ac"], vec!["prepare"], vec!["probe"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_rbr-ac-tracks")).args(&args).output().unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(!output.stderr.is_empty());
    }
}
