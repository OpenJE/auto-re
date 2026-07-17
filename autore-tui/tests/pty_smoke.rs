#[cfg(not(target_os = "windows"))]
#[test]
fn pty_smoke_compiles() {
    use expectrl::Expect;

    let mut session = expectrl::spawn("echo pty_works").expect("spawn should succeed");
    session
        .expect("pty_works")
        .expect("should match output");
}

#[cfg(target_os = "windows")]
#[test]
fn pty_smoke_compiles() {
    eprintln!("skipped: PTY not supported on Windows");
}
