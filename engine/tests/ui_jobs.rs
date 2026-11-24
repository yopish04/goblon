//! Every Job the React UI can send (app/src/presets.js toJob, all presets x shirts x
//! background modes) must deserialize into studiozea_engine::Job and render.
//! Regenerate tests/ui_jobs.json with the node snippet in README if the UI changes.

use studiozea_engine::{run, Job};

#[test]
fn ui_jobs_deserialize_and_render() {
    let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ui_jobs.json"))
        .unwrap();
    let jobs: Vec<Job> = serde_json::from_str(&raw).expect("UI job JSON must match the engine");
    assert_eq!(jobs.len(), 45);
    let src = studiozea_engine::io::load(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/in_art_rgba.png"
    )))
    .unwrap();
    for (i, mut job) in jobs.into_iter().enumerate() {
        job.width_cm = Some(5.0); // keep the test fast
        let out = run(&src, &job);
        assert!(out.checks.ok(), "job {i}: {:?}", out.checks);
        assert!(out.w > 0 && out.h > 0);
    }
}
