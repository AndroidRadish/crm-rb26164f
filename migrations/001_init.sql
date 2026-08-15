-- RB-26164F CRM 初始化 schema
-- 多租户内生：tenant_id 从第一天存在（§9.2 决策）
-- 角色：admin（全局数据/管理成员/自定义阶段/导出）| member（自己名下客户）

-- 枚举类型
DO $$ BEGIN
  CREATE TYPE user_role AS ENUM ('admin', 'member');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
  CREATE TYPE customer_source AS ENUM ('manual', 'excel', 'claim');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
  CREATE TYPE activity_type AS ENUM ('call', 'email', 'meeting', 'note', 'other');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
  CREATE TYPE task_status AS ENUM ('pending', 'done', 'cancelled');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
  CREATE TYPE reminder_channel AS ENUM ('system', 'wecom', 'dingtalk');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
  CREATE TYPE import_status AS ENUM ('pending', 'processing', 'done', 'failed');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- 租户
CREATE TABLE IF NOT EXISTS tenants (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text NOT NULL,
    code        text NOT NULL UNIQUE,
    status      text NOT NULL DEFAULT 'active',
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- 用户
CREATE TABLE IF NOT EXISTS users (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     uuid NOT NULL REFERENCES tenants(id),
    username      text NOT NULL,
    password_hash text NOT NULL,
    role          user_role NOT NULL DEFAULT 'member',
    display_name  text,
    created_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, username)
);

-- 销售阶段（管理员自定义）
CREATE TABLE IF NOT EXISTS stages (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   uuid NOT NULL REFERENCES tenants(id),
    name        text NOT NULL,
    order_index integer NOT NULL DEFAULT 0,
    is_won      boolean NOT NULL DEFAULT false,
    is_lost      boolean NOT NULL DEFAULT false,
    is_active   boolean NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- 客户
CREATE TABLE IF NOT EXISTS customers (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    uuid NOT NULL REFERENCES tenants(id),
    company      text NOT NULL,
    contact_name text,
    phone        text,
    email        text,
    address      text,
    notes        text,
    owner_id     uuid NULL REFERENCES users(id),  -- NULL = 公海池
    stage        uuid NULL REFERENCES stages(id), -- 当前销售阶段
    source       customer_source NOT NULL DEFAULT 'manual',
    duplicate_of uuid NULL REFERENCES customers(id),  -- 去重标记
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

-- 跟进记录（时间轴）
CREATE TABLE IF NOT EXISTS activities (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   uuid NOT NULL REFERENCES tenants(id),
    customer_id uuid NOT NULL REFERENCES customers(id),
    user_id     uuid NOT NULL REFERENCES users(id),
    type        activity_type NOT NULL DEFAULT 'note',
    content     text NOT NULL,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- 待办任务
CREATE TABLE IF NOT EXISTS tasks (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id        uuid NOT NULL REFERENCES tenants(id),
    customer_id      uuid NOT NULL REFERENCES customers(id),
    user_id          uuid NOT NULL REFERENCES users(id),
    title            text NOT NULL,
    due_at           timestamptz NOT NULL,
    status           task_status NOT NULL DEFAULT 'pending',
    reminder_channel reminder_channel NOT NULL DEFAULT 'system',
    created_at       timestamptz NOT NULL DEFAULT now()
);

-- 销售管道条目（阶段流转历史）
CREATE TABLE IF NOT EXISTS pipeline_entries (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       uuid NOT NULL REFERENCES tenants(id),
    customer_id     uuid NOT NULL REFERENCES customers(id),
    stage_id        uuid NOT NULL REFERENCES stages(id),
    entered_at      timestamptz NOT NULL DEFAULT now(),
    exited_at       timestamptz NULL,
    amount_estimate numeric(12,2) NULL
);

-- Excel 导入批次
CREATE TABLE IF NOT EXISTS import_batches (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    uuid NOT NULL REFERENCES tenants(id),
    filename     text NOT NULL,
    user_id      uuid NOT NULL REFERENCES users(id),
    total_rows   integer NOT NULL DEFAULT 0,
    success_rows integer NOT NULL DEFAULT 0,
    failed_rows  integer NOT NULL DEFAULT 0,
    status       import_status NOT NULL DEFAULT 'pending',
    created_at   timestamptz NOT NULL DEFAULT now()
);

-- 审计日志
CREATE TABLE IF NOT EXISTS audit_logs (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   uuid NULL REFERENCES tenants(id),
    user_id     uuid NULL REFERENCES users(id),
    username    text,
    action      text NOT NULL,
    entity_type text NOT NULL,
    entity_id   text,
    detail      text,
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- 索引（多租户查询热点）
CREATE INDEX IF NOT EXISTS idx_customers_tenant ON customers(tenant_id);
CREATE INDEX IF NOT EXISTS idx_customers_owner ON customers(owner_id);
CREATE INDEX IF NOT EXISTS idx_customers_company ON customers(company);
CREATE INDEX IF NOT EXISTS idx_customers_phone ON customers(phone);
CREATE INDEX IF NOT EXISTS idx_customers_stage ON customers(stage);
CREATE INDEX IF NOT EXISTS idx_activities_customer ON activities(customer_id);
CREATE INDEX IF NOT EXISTS idx_tasks_customer ON tasks(customer_id);
CREATE INDEX IF NOT EXISTS idx_tasks_user ON tasks(user_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_pipeline_customer ON pipeline_entries(customer_id);
CREATE INDEX IF NOT EXISTS idx_pipeline_stage ON pipeline_entries(stage_id);
CREATE INDEX IF NOT EXISTS idx_stages_tenant ON stages(tenant_id);
CREATE INDEX IF NOT EXISTS idx_import_batches_tenant ON import_batches(tenant_id);
CREATE INDEX IF NOT EXISTS idx_audit_tenant ON audit_logs(tenant_id);

-- 默认销售阶段（线索→意向→报价→赢单/输单）
INSERT INTO stages (tenant_id, name, order_index, is_won, is_lost)
SELECT t.id, s.name, s.order_index, s.is_won, s.is_lost
FROM tenants t
CROSS JOIN (VALUES
    ('线索', 0, false, false),
    ('意向', 1, false, false),
    ('报价', 2, false, false),
    ('赢单', 3, true, false),
    ('输单', 4, false, true)
) AS s(name, order_index, is_won, is_lost)
WHERE NOT EXISTS (SELECT 1 FROM stages WHERE tenant_id = t.id);
