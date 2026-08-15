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
        .map_err(|e| anyhow::anyhow!("WeCom webhook send failed: {}", e))?;

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
        .map_err(|e| anyhow::anyhow!("DingTalk webhook send failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "DingTalk webhook returned status: {}",
            resp.status()
        ));
    }
    Ok(())
}

/// 根据 reminder_channel 发送提醒
pub async fn send_reminder(channel: &str, title: &str, content: &str) -> anyhow::Result<()> {
    match channel {
        "wecom" => {
            let url = std::env::var("WECOM_WEBHOOK_URL")
                .map_err(|_| anyhow::anyhow!("WECOM_WEBHOOK_URL not configured"))?;
            send_wecom(&url, title, content).await
        }
        "dingtalk" => {
            let url = std::env::var("DINGTALK_WEBHOOK_URL")
                .map_err(|_| anyhow::anyhow!("DINGTALK_WEBHOOK_URL not configured"))?;
            send_dingtalk(&url, title, content).await
        }
        _ => Ok(()), // system 渠道在系统内提醒，无需 webhook
    }
}
