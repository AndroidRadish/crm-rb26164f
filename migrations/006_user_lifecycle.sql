-- 006: 成员生命周期（req-crm-member-lifecycle）
-- is_active=false = 离职/收回账号：禁止登录，且已签发的旧 token 立即失效
-- （鉴权入口 services::auth::require_user 每次按 token sub 查库校验）。
-- 迁移器无版本记录、每次启动全量重放，必须幂等。
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_active boolean NOT NULL DEFAULT true;
