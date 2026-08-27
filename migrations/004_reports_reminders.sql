-- 004: RB-266FB3 新功能点登记 — 报表（crm.reports）+ 跟进提醒（crm.reminders）
-- 粒度按 spec req-rb-266fb3 §2.6 定稿：报表/导出/时间线/提醒逐个登记；
-- 导出复用既有 import.export，时间线复用既有 crm.activities（001 已登记）。
-- 本迁移幂等（迁移器无版本记录，每次启动全量重放，必须可重复执行）。

INSERT INTO feature_keys (key, label, tier) VALUES
    ('crm.reports',   '报表（本月新增/成交率/逾期）', 2),
    ('crm.reminders', '跟进提醒（逾期任务）',        1)
ON CONFLICT (key) DO NOTHING;

-- 存量兼容（与 003 同策略）：对现有租户补种 enabled=true，
-- 防止老客户升级后报表/提醒突然消失。owner 显式写过的记录不覆盖。
INSERT INTO entitlements (scope, scope_id, feature_key, enabled)
SELECT 'tenant', t.id::text, f.key, true
FROM tenants t
CROSS JOIN feature_keys f
WHERE f.key IN ('crm.reports', 'crm.reminders')
ON CONFLICT (scope, scope_id, feature_key) DO NOTHING;
