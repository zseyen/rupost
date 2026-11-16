//! 跨平台进程组管理与超时/取消强杀原语（ProcessGroupGuard）。
//!
//! 收敛 `plugin/driver.rs` 与 `runner/on_failure.rs` 中重复的 Unix PGID
//! 创建/组级 SIGKILL 逻辑，并为优雅停机预留 `tokio::sync::watch` 取消信号入口。

use std::time::Duration;

/// 为子进程配置独立进程组（Unix `process_group(0)`；Windows 由 `kill_on_drop` 兜底）。
pub fn configure_process_group(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = cmd;
    }
}

/// 杀死指定进程组（Unix 强制校验 `pgid > 1 && (pgid as pid_t) > 0`，防止 `pgid == 0` 误杀当前进程组）。
pub fn kill_process_group_pid(pgid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pgid) = pgid.filter(|&id| id > 1 && (id as libc::pid_t) > 0) {
        // SAFETY: killpg 对任意正 pgid 调用都是内存安全的纯系统调用；
        // 已过滤 <= 1 与符号位溢出值，进程组由本进程 spawn 时通过 process_group(0) 创建。
        unsafe {
            libc::killpg(pgid as libc::pid_t, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pgid;
    }
}

/// 进程组生命周期守卫：Drop 时若尚未 `disarm()`，自动对整个进程组发送 `SIGKILL`。
#[derive(Debug)]
pub struct ProcessGroupGuard {
    pgid: Option<u32>,
    armed: bool,
}

impl ProcessGroupGuard {
    /// 创建一个已武装的进程组守卫。
    pub fn new(pgid: Option<u32>) -> Self {
        Self { pgid, armed: true }
    }

    /// 进程正常退出后解除武装，防止 PID 被内核复用后误杀。
    pub fn disarm(&mut self) {
        self.armed = false;
    }

    /// 立即向进程组发送 `SIGKILL` 并解除武装。
    pub fn kill_now(&mut self) {
        if self.armed {
            kill_process_group_pid(self.pgid);
            self.armed = false;
        }
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        if self.armed {
            kill_process_group_pid(self.pgid);
        }
    }
}

/// 等待异步 Future 完成，同时受超时与可选取消信号（`watch::Receiver<bool>`）约束。
pub enum WaitOutcome<T> {
    Completed(T),
    TimedOut,
    Cancelled,
}

pub async fn wait_with_timeout_and_cancel<F, T>(
    fut: F,
    timeout: Duration,
    cancel: Option<&mut tokio::sync::watch::Receiver<bool>>,
) -> WaitOutcome<T>
where
    F: std::future::Future<Output = T>,
{
    match cancel {
        Some(rx) => {
            if *rx.borrow() {
                return WaitOutcome::Cancelled;
            }
            tokio::select! {
                res = tokio::time::timeout(timeout, fut) => match res {
                    Ok(v) => WaitOutcome::Completed(v),
                    Err(_) => WaitOutcome::TimedOut,
                },
                _ = async {
                    match rx.wait_for(|&cancelled| cancelled).await {
                        Ok(_) => {}
                        Err(_) => std::future::pending::<()>().await,
                    }
                } => WaitOutcome::Cancelled,
            }
        }
        None => match tokio::time::timeout(timeout, fut).await {
            Ok(v) => WaitOutcome::Completed(v),
            Err(_) => WaitOutcome::TimedOut,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_group_guard_disarm_and_kill_now() {
        let mut g1 = ProcessGroupGuard::new(None);
        assert!(g1.armed);
        g1.disarm();
        assert!(!g1.armed);
        g1.kill_now();
        assert!(!g1.armed);

        let mut g2 = ProcessGroupGuard::new(None);
        g2.kill_now();
        assert!(!g2.armed);
    }

    #[tokio::test]
    async fn test_wait_with_timeout_and_cancel_handles_cancel_and_timeout() {
        let (_tx, mut rx) = tokio::sync::watch::channel(true);
        let res =
            wait_with_timeout_and_cancel(async { 42 }, Duration::from_secs(1), Some(&mut rx)).await;
        assert!(matches!(res, WaitOutcome::Cancelled));

        let res_timeout = wait_with_timeout_and_cancel(
            tokio::time::sleep(Duration::from_secs(5)),
            Duration::from_millis(10),
            None,
        )
        .await;
        assert!(matches!(res_timeout, WaitOutcome::TimedOut));
    }

    #[test]
    fn test_kill_process_group_pid_zero_is_safe_noop() {
        // 若未拦截 pgid == 0，当前测试进程组会收到 SIGKILL
        kill_process_group_pid(Some(0));
        kill_process_group_pid(Some(1));
        let mut guard = ProcessGroupGuard::new(Some(0));
        guard.kill_now();
    }

    #[cfg(unix)]
    #[test]
    fn test_process_group_guard_drop_kills_real_spawned_process_group() {
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", "sleep 30"]);
        configure_process_group(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let pgid = child.id();

        {
            let _guard = ProcessGroupGuard::new(Some(pgid));
        }

        let status = child.wait().unwrap();
        assert!(!status.success(), "Drop 必须真实杀死子进程组");
    }

    #[tokio::test]
    async fn test_wait_with_timeout_and_cancel_in_flight_branches() {
        // 1. 运行中收到 true 取消信号 -> Cancelled
        let (tx, mut rx) = tokio::sync::watch::channel(false);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = tx.send(true);
        });
        let res = wait_with_timeout_and_cancel(
            tokio::time::sleep(Duration::from_secs(5)),
            Duration::from_secs(5),
            Some(&mut rx),
        )
        .await;
        assert!(matches!(res, WaitOutcome::Cancelled));

        // 2. Sender 被 drop 且任务正常完成 -> Completed(42)（绝不能误判 Cancelled）
        let (tx2, mut rx2) = tokio::sync::watch::channel(false);
        drop(tx2);
        let res2 =
            wait_with_timeout_and_cancel(async { 42 }, Duration::from_secs(1), Some(&mut rx2))
                .await;
        assert!(matches!(res2, WaitOutcome::Completed(42)));
    }
}
