use anyhow::{Context, Result};
use opshub_core::ansi;
use opshub_core::event::{Event, EventKind};
use opshub_core::Storage;
use portable_pty::{CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{mpsc as stdmpsc, Arc, Mutex};
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::profile::{AgentProfile, WinSize};

/// Fan-out copy of every byte observed on the PTY plus lifecycle signals.
/// Subscribers drive UI, storage persistence, cost parsers, etc.
#[derive(Debug, Clone)]
pub enum RunnerEvent {
    /// Raw bytes read from the PTY master (mixture of stdout + stderr - PTYs
    /// merge them unless the child cooperates).
    Output(bytes::Bytes),
    /// Child process exited with the given status (portable-pty exposes a u32;
    /// we keep it as-is).
    Exited(u32),
}

pub struct RunningAgent {
    pub session_id: String,
    pub profile: AgentProfile,
    pub events: broadcast::Sender<RunnerEvent>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub win: WinSize,
}

impl RunningAgent {
    pub fn subscribe(&self) -> broadcast::Receiver<RunnerEvent> {
        self.events.subscribe()
    }

    /// Forward bytes to the child's stdin through the PTY master.
    pub fn write_input(&self, bytes: &[u8]) -> Result<()> {
        let guard = self.master.lock().expect("pty master poisoned");
        let mut writer = guard.take_writer().context("take pty writer")?;
        writer.write_all(bytes).context("write pty stdin")?;
        Ok(())
    }

    pub fn resize(&mut self, new: WinSize) -> Result<()> {
        let guard = self.master.lock().expect("pty master poisoned");
        guard
            .resize(PtySize {
                rows: new.rows,
                cols: new.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("pty resize")?;
        drop(guard);
        self.win = new;
        Ok(())
    }
}

/// Spawn `profile` inside a PTY and wire every readable byte into:
///   1. a broadcast channel (`RunnerEvent::Output`) for in-process subscribers
///   2. the provided `Storage` as `event(kind='stdout')` rows
///
/// Exit status also lands in both sinks (as `RunnerEvent::Exited` and a
/// `session.ended_at` update).
pub fn spawn_agent(profile: AgentProfile, storage: Storage, win: WinSize) -> Result<RunningAgent> {
    let session_id = ulid::Ulid::new().to_string();

    storage
        .insert_agent(
            &profile.id,
            &profile.kind,
            &serde_json::to_string(&profile)?,
        )
        .context("insert_agent")?;
    storage
        .start_session(
            &session_id,
            &profile.id,
            profile.cwd.as_deref().and_then(|p| p.to_str()),
            None,
            None,
        )
        .context("start_session")?;

    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: win.rows,
            cols: win.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("openpty")?;

    let mut cmd = CommandBuilder::new(&profile.command);
    cmd.args(profile.args.iter().map(|s| s.as_str()));
    if let Some(cwd) = profile.cwd.as_ref() {
        cmd.cwd(cwd);
    }
    for (k, v) in &profile.env {
        cmd.env(k, v);
    }

    let mut child = pair.slave.spawn_command(cmd).context("spawn child")?;
    // drop slave in this process so read() on master returns EOF when the child exits.
    drop(pair.slave);

    let master = Arc::new(Mutex::new(pair.master));
    let reader = {
        let guard = master.lock().expect("pty master poisoned");
        guard.try_clone_reader().context("clone pty reader")?
    };

    let (tx, _rx0) = broadcast::channel::<RunnerEvent>(1024);
    let (byte_tx, byte_rx) = stdmpsc::channel::<Vec<u8>>();

    // Blocking reader thread: PTY reads are synchronous, so we stay off the
    // tokio runtime entirely. We hold the JoinHandle so the waiter below can
    // drain any queued bytes before declaring the agent "Exited".
    let reader_handle = std::thread::Builder::new()
        .name(format!("opshub-pty-{}", profile.id))
        .spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if byte_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "pty read error");
                        break;
                    }
                }
            }
            debug!("pty reader exited");
        })
        .context("spawn pty reader thread")?;

    // Bridge std mpsc -> tokio broadcast + storage. Kept on a dedicated thread
    // (not `spawn_blocking`) so its JoinHandle can be awaited synchronously
    // from the waiter; this matters because subscribers may call
    // `std::process::exit` on `Exited` and we don't want to race the flush.
    let tx_for_bridge = tx.clone();
    let session_for_bridge = session_id.clone();
    let storage_for_bridge = storage.clone();
    let bridge_handle = std::thread::Builder::new()
        .name(format!("opshub-bridge-{}", profile.id))
        .spawn(move || {
            while let Ok(bytes) = byte_rx.recv() {
                let text = ansi::strip(&bytes);
                let ev = Event::new(&session_for_bridge, EventKind::Stdout)
                    .with_payload(bytes.clone())
                    .with_text(text);
                if let Err(e) = storage_for_bridge.insert_event(&ev) {
                    warn!(error = %e, "storage insert_event failed");
                }
                let _ = tx_for_bridge.send(RunnerEvent::Output(bytes::Bytes::from(bytes)));
            }
            debug!("bridge exited");
        })
        .context("spawn bridge thread")?;

    // Child waiter: wait on the process, then drain reader + bridge before
    // emitting Exited. Ordering: child -> reader EOF -> byte_tx dropped ->
    // bridge recv Err -> bridge exits -> Exited event fires.
    let tx_for_exit = tx.clone();
    let session_for_exit = session_id.clone();
    let storage_for_exit = storage.clone();
    std::thread::Builder::new()
        .name(format!("opshub-wait-{}", profile.id))
        .spawn(move || {
            let status = child.wait();
            let _ = reader_handle.join();
            let _ = bridge_handle.join();
            let code = match status {
                Ok(s) => s.exit_code(),
                Err(e) => {
                    warn!(error = %e, "child wait failed");
                    u32::MAX
                }
            };
            let _ = storage_for_exit.end_session(&session_for_exit, Some(code as i32));
            let _ = tx_for_exit.send(RunnerEvent::Exited(code));
        })
        .context("spawn child waiter")?;

    Ok(RunningAgent {
        session_id,
        profile,
        events: tx,
        master,
        win,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use opshub_core::event::EventKind;
    use tokio::time::{timeout, Duration};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn echo_roundtrip_persists_events() {
        let storage = Storage::open_in_memory().unwrap();
        let profile = AgentProfile {
            id: "echo-test".into(),
            kind: "generic".into(),
            command: "/bin/sh".into(),
            args: vec!["-c".into(), "printf 'opshub-ok'".into()],
            cwd: None,
            env: vec![],
        };
        let running = spawn_agent(profile, storage.clone(), WinSize::default()).unwrap();
        let session_id = running.session_id.clone();
        let mut rx = running.subscribe();

        let saw_exit = timeout(Duration::from_secs(5), async {
            loop {
                match rx.recv().await {
                    Ok(RunnerEvent::Exited(c)) => return c,
                    Ok(RunnerEvent::Output(_)) => continue,
                    Err(_) => return u32::MAX,
                }
            }
        })
        .await
        .expect("did not observe child exit within 5s");
        assert_eq!(saw_exit, 0, "echo should exit 0");

        // Give bridge task a beat to flush the last stdout chunk before the
        // child's eof closes the std mpsc channel.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let hits = storage.search("opshub-ok", 10).unwrap();
        assert!(
            !hits.is_empty(),
            "expected FTS to find 'opshub-ok' in stdout"
        );
        assert!(hits.iter().any(|(_, sid, _, _)| sid == &session_id));

        let n = storage
            .count_events(&session_id, EventKind::Stdout)
            .unwrap();
        assert!(n >= 1, "expected at least one stdout event, got {n}");
    }
}
