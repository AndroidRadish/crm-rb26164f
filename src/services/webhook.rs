//! 企微/钉钉 webhook 提醒服务
//!
//! 通过环境变量配置 webhook URL：
//! - WECOM_WEBHOOK_URL：企业微信机器人 webhook
//! - DINGTALK_WEBHOOK_URL：钉钉机器人 webhook
//!
//! 任务到期时调用 send_reminder 发送提醒。

use serde_json::json;

/// 发送企微 webhook 提醒
pub async fn send_wecom(webhook_url: &str, title: &str, content: &str) -> anyhow::Result<()> {
    let body = json!({
        "msgtype": "text",
        "text": {
            "content": format!("【CRM 待办提醒】\n{}\n{}", title, content)
        }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(webhook_url)
        .json(&body)
        .send()
        .await
        // URL 里带机器人 key（等同凭据）：错误信息去掉 URL 再向上抛，
        // 避免调度器/调用方把 URL 明文写进日志（req-crm-reminder-scheduler 约束）。
        .map_err(|e| anyhow::anyhow!("WeCom webhook send failed: {}", e.without_url()))?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "WeCom webhook returned status: {}",
            resp.status()
        ));
    }
    Ok(())
}

/// 发送钉钉 webhook 提醒
pub async fn send_dingtalk(webhook_url: &str, title: &str, content: &str) -> anyhow::Result<()> {
    let body = json!({
        "msgtype": "text",
        "text": {
            "content": format!("【CRM 待办提醒】\n{}\n{}", title, content)
        }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(webhook_url)
        .json(&body)
        .send()
        .await
        // 同 send_wecom：错误信息不得携带含 key 的 URL 明文
        .map_err(|e| anyhow::anyhow!("DingTalk webhook send failed: {}", e.without_url()))?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "DingTalk webhook returned status: {}",
            resp.status()
        ));
    }
    Ok(())
}

/// 根据 reminder_channel 发送提醒
pub async fn send_reminder(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    channel: &str,
    title: &str,
    content: &str,
) -> anyhow::Result<()> {
    let key = match channel {
        "wecom" => "wecom_webhook_url",
        "dingtalk" => "dingtalk_webhook_url",
        _ => return Ok(()), // system 渠道在系统内提醒，无需 webhook
    };
    // DB 设置优先，环境变量兜底
    let url = crate::handlers::settings::get_setting(pool, tenant_id, key)
        .await
        .ok_or_else(|| anyhow::anyhow!("{key} 未配置（后台 设置 页或环境变量）"))?;
    match channel {
        "wecom" => send_wecom(&url, title, content).await,
        "dingtalk" => send_dingtalk(&url, title, content).await,
        _ => Ok(()),
    }
}
