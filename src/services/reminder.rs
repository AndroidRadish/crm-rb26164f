//! 任务到期提醒调度器（req-crm-reminder-scheduler）
//!
//! 每 60s 扫描 `tasks WHERE status='pending' AND due_at <= now() AND notified_at IS NULL`：
//! 1. 先 `UPDATE tasks SET notified_at = now() WHERE id = $1 AND notified_at IS NULL RETURNING id`
//!    抢占标记——只有抢到的实例才发送，多实例/并发扫描也不会重复提醒；
//! 2. 抢占成功再调 [`crate::services::webhook::send_reminder`]（system 渠道恒 Ok，
//!    系统内提醒由既有逾期报表承担）；
//! 3. 发送失败把 `notified_at` 清回 NULL 交给下一轮重试，并 error 日志记录任务 id 与错误
//!    （不打印 webhook URL：URL 含机器人 key，等同凭据）。
//!
//! 调度循环由 main 以 `tokio::spawn` 挂载，进程/axum 退出时随之结束，无需额外 draining。

use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// 扫描间隔：60s（spec §需求 2）
pub const SCAN_INTERVAL: Duration = Duration::from_secs(60);

/// 到期任务行（JOIN 客户取公司名拼进提醒正文）
#[derive(Debug, sqlx::FromRow)]
struct DueTask {
    id: Uuid,
    tenant_id: Uuid,
    reminder_channel: String,
    title: String,
    due_at: chrono::DateTime<chrono::Utc>,
    customer_company: String,
}

/// 后台调度循环：随进程退出（axum serve 结束 / runtime 关闭）一起结束。
pub async fn run_scheduler(pool: PgPool) {
    let mut ticker = tokio::time::interval(SCAN_INTERVAL);
    // 单轮耗时超过 60s 时跳过错过的 tick，避免补偿式连发
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        match run_once(&pool).await {
            Ok(0) => {}
            Ok(n) => tracing::info!("到期提醒：本轮成功发送 {} 条", n),
            // 扫描/DB 失败只记日志，下一轮继续（不能因一次故障让调度器退出）
            Err(e) => tracing::error!("到期提醒扫描失败，下一轮重试: {}", e),
        }
    }
}

/// 执行一轮扫描与发送，返回本轮「抢占成功且发送成功」的条数。
///
/// 暴露为 pub 以便集成测试绕过 60s 等待直接驱动单轮。
pub async fn run_once(pool: &PgPool) -> anyhow::Result<usize> {
    let due: Vec<DueTask> = sqlx::query_as::<_, DueTask>(
        "SELECT t.id, t.tenant_id, t.reminder_channel::text AS reminder_channel, \
                t.title, t.due_at, c.company AS customer_company \
         FROM tasks t \
         JOIN customers c ON c.id = t.customer_id \
         WHERE t.status = 'pending' AND t.due_at <= now() AND t.notified_at IS NULL \
         ORDER BY t.due_at ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut sent = 0usize;
    for task in due {
        // 幂等抢占：并发实例中只有一个能拿到 RETURNING 行
        if !claim(pool, task.id).await? {
            continue;
        }

        let content = format!(
            "客户：{}\n截止时间：{}",
            task.customer_company,
            task.due_at.to_rfc3339()
        );
        match crate::services::webhook::send_reminder(
            pool,
            task.tenant_id,
            &task.reminder_channel,
            &task.title,
            &content,
        )
        .await
        {
            Ok(()) => {
                sent += 1;
                tracing::info!("到期提醒已发送: task={} channel={}", task.id, task.reminder_channel);
            }
            Err(e) => {
                // 失败重试：清回 NULL 让下一轮重新扫描；错误只带任务 id 与错误描述，
                // send_reminder 的错误信息只含 settings key/状态码，不含 URL 明文。
                tracing::error!("到期提醒发送失败，下轮重试: task={} err={}", task.id, e);
                if let Err(reset_err) = reset_notified_at(pool, task.id).await {
                    tracing::error!(
                        "到期提醒失败后重置 notified_at 失败: task={} err={}",
                        task.id,
                        reset_err
                    );
                }
            }
        }
    }
    Ok(sent)
}

/// 幂等抢占：仅当 notified_at 仍为 NULL 时写入 now() 并返回是否抢到。
async fn claim(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let claimed: Option<Uuid> = sqlx::query_scalar(
        "UPDATE tasks SET notified_at = now() WHERE id = $1 AND notified_at IS NULL RETURNING id",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(claimed.is_some())
}

/// 发送失败后清回 NULL（保留 status/due_at 不变，仅释放提醒标记）。
async fn reset_notified_at(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    let res = sqlx::query("UPDATE tasks SET notified_at = NULL WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        tracing::warn!("到期提醒重置 notified_at 时任务已不存在: task={}", id);
    }
    Ok(())
}
