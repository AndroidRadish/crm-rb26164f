-- 005: 任务到期提醒调度器（req-crm-reminder-scheduler）
-- notified_at 既是「已提醒」时间戳，也是调度器的幂等抢占标记：
-- NULL = 尚未提醒；非 NULL = 已抢占/已发送（发送失败由调度器清回 NULL 重试）。
-- 迁移器无版本记录、每次启动全量重放，必须幂等。
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS notified_at timestamptz NULL;
