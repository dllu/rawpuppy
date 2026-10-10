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

#[test]
fn cli_export_rejects_original_paths_and_aliases_even_with_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("original.png");
    image::RgbImage::from_pixel(2, 2, image::Rgb([40, 100, 180]))
        .save(&input)
        .unwrap();
    let original = std::fs::read(&input).unwrap();
    let recipe = sidecar::path_for(&input);
    sidecar::save(&recipe, &Edits::default()).unwrap();
    let saved = std::fs::read(&recipe).unwrap();
    let outputs = vec![
        input.clone(),
        directory.path().join(".").join("original.png"),
    ];
    #[cfg(unix)]
    let outputs = {
        let mut outputs = outputs;
        let alias = directory.path().join("alias.tiff");
        std::os::unix::fs::symlink(&input, &alias).unwrap();
        outputs.push(alias);
        outputs
    };
    for output in outputs {
        for overwrite in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_rawpuppy"));
            command
                .args(["--backend", "cpu", "export"])
                .arg(&input)
                .arg(&output);
            if overwrite {
                command.arg("--overwrite");
            }
            let result = command.output().unwrap();
            assert!(
                !result.status.success(),
                "Export accepted its original destination"
            );
            assert!(
                String::from_utf8_lossy(&result.stderr).contains("cannot overwrite the original")
            );
            assert_eq!(std::fs::read(&input).unwrap(), original);
            assert_eq!(std::fs::read(&recipe).unwrap(), saved);
        }
    }
    #[cfg(unix)]
    assert!(
        std::fs::symlink_metadata(directory.path().join("alias.tiff"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn cli_export_write_failure_does_not_publish_or_replace_files() {
    use std::os::unix::process::CommandExt;
    for extension in ["png", "jpg", "tiff", "exr"] {
        for replace in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let input = directory.path().join("input.png");
            image::RgbImage::from_pixel(2, 2, image::Rgb([40, 100, 180]))
                .save(&input)
                .unwrap();
            let original = std::fs::read(&input).unwrap();
            let output = directory.path().join(format!("output.{extension}"));
            let prior = b"existing export must survive failed replacement";
            if replace {
                std::fs::write(&output, prior).unwrap();
            }
            let filenames = || {
                let mut names: Vec<_> = std::fs::read_dir(directory.path())
                    .unwrap()
                    .map(|e| e.unwrap().file_name())
                    .collect();
                names.sort();
                names
            };
            let before = filenames();
            let mut command = Command::new(env!("CARGO_BIN_EXE_rawpuppy"));
            command
                .args(["--backend", "cpu", "export"])
                .arg(&input)
                .arg(&output)
                .args(["--color-space", "linear-srgb"]);
            if replace {
                command.arg("--overwrite");
            }
            // SAFETY: only async-signal-safe operations run in the owned child
            // before exec; the parent and other processes keep their limits.
            unsafe {
                command.pre_exec(|| {
                    let limit = libc::rlimit {
                        rlim_cur: 64,
                        rlim_max: 64,
                    };
                    if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0
                        || libc::signal(libc::SIGXFSZ, libc::SIG_IGN) == libc::SIG_ERR
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let result = command.output().unwrap();
            assert!(
                !result.status.success(),
                "{extension} export reported success despite failed writes (replace={replace})"
            );
            assert!(
                String::from_utf8_lossy(&result.stderr).contains("File too large"),
                "Expected file-size write failure, got {}",
                String::from_utf8_lossy(&result.stderr)
            );
            if replace {
                assert_eq!(std::fs::read(&output).unwrap(), prior);
            } else {
                assert!(!output.exists());
            }
            assert_eq!(std::fs::read(input).unwrap(), original);
            assert_eq!(filenames(), before, "Failed export left staging files");
        }
    }
}
