-- 003: 功能分级（entitlement 层打样）— 依据 planning/feature-tiering-pattern.md
-- 缺省规则：entitlements 无记录 = 按 feature_keys.tier 默认档（tier=1 默认开，tier>=2 默认关）
-- 本迁移幂等（迁移器无版本记录，每次启动全量重放，必须可重复执行）

-- 功能键登记表（键名登记制：代码里出现的键必须先在此登记）
CREATE TABLE IF NOT EXISTS feature_keys (
    key        text PRIMARY KEY,
    label      text NOT NULL,
    tier       integer NOT NULL CHECK (tier IN (1, 2, 3)),  -- 1基础 2专业 3旗舰
    created_at timestamptz NOT NULL DEFAULT now()
);

-- 授权表（多租户挂 tenant）
CREATE TABLE IF NOT EXISTS entitlements (
    scope       text NOT NULL DEFAULT 'tenant',
    scope_id    text NOT NULL,
    feature_key text NOT NULL REFERENCES feature_keys(key),
    enabled     boolean NOT NULL DEFAULT true,
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (scope, scope_id, feature_key)
);

-- 登记：3 个打样分级功能 + 既有基础功能（tier=1）
INSERT INTO feature_keys (key, label, tier) VALUES
    ('crm.customers',  '客户库',          1),
    ('crm.kanban',     '销售看板',        1),
    ('crm.activities', '跟进记录',        1),
    ('crm.tasks',      '待办任务',        1),
    ('crm.funnel',     '销售漏斗',        1),
    ('crm.audit',      '审计日志',        1),
    ('pool.claim',     '公海认领/释放',   2),
    ('import.export',  '导入/导出',       2),
    ('notify.webhook', '企微/钉钉提醒',   3)
ON CONFLICT (key) DO NOTHING;

-- 存量兼容（打样期）：对现有租户把 3 个分级功能补种 enabled=true，
-- 防止老客户一觉醒来功能没了。已存在记录（如 owner 手工关掉）不覆盖。
-- 注：迁移器每次启动重放本文件，新建租户会在下次启动时被补种覆盖；
-- owner 显式写过的记录因 ON CONFLICT DO NOTHING 不受影响。
INSERT INTO entitlements (scope, scope_id, feature_key, enabled)
SELECT 'tenant', t.id::text, f.key, true
FROM tenants t
CROSS JOIN feature_keys f
WHERE f.key IN ('pool.claim', 'import.export', 'notify.webhook')
ON CONFLICT (scope, scope_id, feature_key) DO NOTHING;
