//! Runs `echo hi; false` in zsh through a PTY and prints the snapshot and the
//! command blocks. No window involved.

use std::thread;
use std::time::{Duration, Instant};

use term_session::{Session, StartupTimer};

fn main() -> anyhow::Result<()> {
    let timer = StartupTimer::start();
    let session = Session::spawn(100, 24)?;

    // Wait for the first prompt to be drawn. Headless, that stands in for "first frame".
    wait_for(Duration::from_secs(10), || {
        session
            .snapshot()
            .text_lines()
            .iter()
            .any(|l| !l.is_empty())
    })?;
    timer.first_frame_presented();

    session.write(b"echo hi; false\r")?;

    wait_for(Duration::from_secs(10), || {
        let blocks = session.blocks();
        blocks.iter().any(|b| b.exit_code.is_some())
    })?;
    // Give the second prompt a moment to draw after the D mark.
    thread::sleep(Duration::from_millis(200));

    let snapshot = session.snapshot();
    println!("--- snapshot ({}x{}) ---", snapshot.columns, snapshot.rows);
    for (i, line) in snapshot.text_lines().iter().enumerate() {
        println!("{i:>2} | {line}");
    }
    println!(
        "cursor: line {} column {} visible {} shape {:?}",
        snapshot.cursor.line,
        snapshot.cursor.column,
        snapshot.cursor.visible,
        snapshot.cursor.shape
    );
    println!("damage: {:?}", snapshot.damage);

    println!("--- blocks ---");
    for (i, block) in session.blocks().iter().enumerate() {
        println!(
            "#{i} command={:?} exit_code={:?} duration={:?}",
            block.command,
            block.exit_code,
            block.duration()
        );
    }
    Ok(())
}

fn wait_for(timeout: Duration, mut ready: impl FnMut() -> bool) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    while !ready() {
        anyhow::ensure!(Instant::now() < deadline, "timed out waiting for the shell");
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
