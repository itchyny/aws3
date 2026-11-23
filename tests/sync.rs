// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::Env;

#[test]
fn sync_upload() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("src/a.txt", "a");
    env.write("src/sub/b.txt", "b");
    let result = env
        .run(&["sync", "src", "s3://bucket/dir", "--no-progress"])
        .success();
    assert_eq!(
        result.sorted_lines(),
        [
            "upload: src/a.txt to s3://bucket/dir/a.txt",
            "upload: src/sub/b.txt to s3://bucket/dir/sub/b.txt",
        ]
    );
    // Nothing is synced when the destination is up to date.
    let result = env
        .run(&["sync", "src", "s3://bucket/dir", "--no-progress"])
        .success();
    assert_eq!(result.stdout, "");

    // Changed sizes are synced, and deleted files are deleted with --delete.
    env.write("src/a.txt", "changed");
    std::fs::remove_file(env.path("src/sub/b.txt")).unwrap();
    let result = env
        .run(&[
            "sync",
            "src",
            "s3://bucket/dir",
            "--delete",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.sorted_lines(),
        [
            "delete: s3://bucket/dir/sub/b.txt",
            "upload: src/a.txt to s3://bucket/dir/a.txt",
        ]
    );
    assert_eq!(
        env.object_content("bucket", "dir/a.txt").unwrap(),
        b"changed"
    );
    assert_eq!(env.object_content("bucket", "dir/sub/b.txt"), None);
}

#[test]
fn sync_download() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "dir/a.txt", "a");
    env.object("bucket", "dir/b.txt", "b");
    let result = env
        .run(&["sync", "s3://bucket/dir", "out", "--no-progress"])
        .success();
    assert_eq!(
        result.sorted_lines(),
        [
            "download: s3://bucket/dir/a.txt to out/a.txt",
            "download: s3://bucket/dir/b.txt to out/b.txt",
        ]
    );
    // The downloaded files have the modified times of the objects.
    let result = env
        .run(&["sync", "s3://bucket/dir", "out", "--no-progress"])
        .success();
    assert_eq!(result.stdout, "");

    env.write("out/extra.txt", "x");
    let result = env
        .run(&[
            "sync",
            "s3://bucket/dir",
            "out",
            "--delete",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.lines(), ["delete: out/extra.txt"]);
    assert!(!env.path("out/extra.txt").exists());
}

#[test]
fn sync_size_only_and_no_overwrite() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("src/a.txt", "aaa");
    env.run(&["sync", "src", "s3://bucket/", "--no-progress"])
        .success();
    env.write("src/a.txt", "bbb");
    let result = env
        .run(&[
            "sync",
            "src",
            "s3://bucket/",
            "--size-only",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.stdout, "");
    env.write("src/a.txt", "cccc");
    let result = env
        .run(&[
            "sync",
            "src",
            "s3://bucket/",
            "--no-overwrite",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.stdout, "");
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"aaa");
}

#[test]
fn sync_s3_to_s3() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "src/a.txt", "a");
    env.object("bucket", "src/sub/b.txt", "b");
    env.object("bucket", "dest/stale.txt", "x");
    let result = env
        .run(&[
            "sync",
            "s3://bucket/src",
            "s3://bucket/dest",
            "--delete",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.sorted_lines(),
        [
            "copy: s3://bucket/src/a.txt to s3://bucket/dest/a.txt",
            "copy: s3://bucket/src/sub/b.txt to s3://bucket/dest/sub/b.txt",
            "delete: s3://bucket/dest/stale.txt",
        ]
    );
    assert_eq!(
        env.object_content("bucket", "dest/sub/b.txt").unwrap(),
        b"b"
    );
    assert_eq!(env.object_content("bucket", "dest/stale.txt"), None);
    // The deletions are sent in a batch of DeleteObjects.
    let requests = env.requests();
    assert!(
        requests
            .iter()
            .any(|request| request.method == "POST" && request.has_query("delete"))
    );
    assert!(requests.iter().all(|request| request.method != "DELETE"));

    let result = env
        .run(&[
            "sync",
            "s3://bucket/src",
            "s3://bucket/dry",
            "--dryrun",
            "--exclude",
            "sub/*",
        ])
        .success();
    assert_eq!(
        result.lines(),
        ["(dryrun) copy: s3://bucket/src/a.txt to s3://bucket/dry/a.txt"]
    );
}

#[test]
fn sync_exact_timestamps() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "dir/a.txt", "a");
    env.run(&["sync", "s3://bucket/dir", "out", "--no-progress"])
        .success();
    // The local file of the same size but an older time is not synced by the
    // default, but synced with --exact-timestamps.
    let file = std::fs::File::options()
        .write(true)
        .open(env.path("out/a.txt"))
        .unwrap();
    let older = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    file.set_modified(older).unwrap();
    let result = env
        .run(&["sync", "s3://bucket/dir", "out", "--no-progress"])
        .success();
    assert_eq!(result.stdout, "");
    let result = env
        .run(&[
            "sync",
            "s3://bucket/dir",
            "out",
            "--exact-timestamps",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.lines(),
        ["download: s3://bucket/dir/a.txt to out/a.txt"]
    );
}

#[test]
fn sync_case_conflict() {
    let env = Env::new();
    // Case conflicts are detected only on case-insensitive file systems.
    env.write("probe", "");
    if !env.path("PROBE").exists() {
        return;
    }
    env.bucket("bucket");
    env.object("bucket", "dir/A.txt", "upper");
    env.write("out/a.txt", "lower");
    let result = env
        .run(&[
            "sync",
            "s3://bucket/dir",
            "out",
            "--case-conflict",
            "skip",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.stdout, "");
    assert!(
        result
            .stderr
            .starts_with("warning: Skipping bucket/dir/A.txt -> "),
        "{}",
        result.stderr
    );
    let result = env.run(&[
        "cp",
        "s3://bucket/dir",
        "out",
        "--recursive",
        "--case-conflict",
        "error",
    ]);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .starts_with("fatal error: Failed to download bucket/dir/A.txt -> "),
        "{}",
        result.stderr
    );
    let result = env
        .run(&[
            "cp",
            "s3://bucket/dir",
            "out",
            "--recursive",
            "--case-conflict",
            "warn",
            "--no-progress",
        ])
        .success();
    assert!(
        result
            .stderr
            .starts_with("warning: Downloading bucket/dir/A.txt -> "),
        "{}",
        result.stderr
    );
}

#[test]
fn sync_case_conflict_delete() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "dir/a.txt", "new");
    // The local file whose name differs only by case is deleted before the
    // download, which is the same file on case-insensitive file systems.
    for mode in ["ignore", "skip", "warn", "error"] {
        env.write("out/A.txt", "old");
        let result = env
            .run(&[
                "sync",
                "s3://bucket/dir",
                "out",
                "--delete",
                "--case-conflict",
                mode,
                "--no-progress",
            ])
            .success();
        assert_eq!(
            result.lines(),
            [
                "delete: out/A.txt",
                "download: s3://bucket/dir/a.txt to out/a.txt"
            ],
            "{mode}"
        );
        assert_eq!(result.stderr, "", "{mode}");
        let names: Vec<_> = std::fs::read_dir(env.path("out"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["a.txt"], "{mode}");
        assert_eq!(env.read("out/a.txt"), b"new", "{mode}");
        std::fs::remove_file(env.path("out/a.txt")).unwrap();
    }
}

#[test]
fn sync_case_conflict_in_flight() {
    let env = Env::new();
    // The objects whose keys differ only by case are stored only on
    // case-sensitive file systems, where the conflicts are detected only
    // with the keys being downloaded.
    env.write("probe", "");
    if env.path("PROBE").exists() {
        return;
    }
    env.bucket("bucket");
    env.object("bucket", "dir/A.txt", "upper");
    env.object("bucket", "dir/a.txt", "lower");
    let result = env
        .run(&[
            "sync",
            "s3://bucket/dir",
            "out",
            "--case-conflict",
            "skip",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.lines(),
        ["download: s3://bucket/dir/A.txt to out/A.txt"]
    );
    assert!(
        result
            .stderr
            .starts_with("warning: Skipping bucket/dir/a.txt -> "),
        "{}",
        result.stderr
    );
}
