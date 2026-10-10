use rawpuppy::{edits::Edits, sidecar};
use std::process::Command;

#[test]
fn cli_save_only_writes_its_own_sidecar_suffix() {
    let directory = tempfile::tempdir().unwrap();
    let recipe = directory.path().join("edits.json");
    std::fs::write(&recipe, serde_json::to_vec(&Edits::default()).unwrap()).unwrap();
    for name in ["photo.raf", "photo.raf.xmp", "edits.json"] {
        let output = directory.path().join(name);
        if output != recipe {
            std::fs::write(&output, b"immutable original or foreign sidecar").unwrap();
        }
        let original = std::fs::read(&output).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_rawpuppy"))
            .args(["save"])
            .arg(&recipe)
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            !result.status.success(),
            "Save accepted protected destination {name}"
        );
        assert_eq!(
            std::fs::read(&output).unwrap(),
            original,
            "Save replaced {name}"
        );
    }
    let output = directory.path().join("photo.raf.rawpuppy.xmp");
    let result = Command::new(env!("CARGO_BIN_EXE_rawpuppy"))
        .arg("save")
        .arg(&recipe)
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(sidecar::load(&output).unwrap(), Edits::default());
}
