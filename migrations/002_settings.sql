-- 002: 租户级设置（webhook URL 等后台可配项）
CREATE TABLE IF NOT EXISTS settings (
    tenant_id  uuid NOT NULL REFERENCES tenants(id),
    key        text NOT NULL,
    value      text NOT NULL DEFAULT '',
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, key)
);
