#!/usr/bin/env bash
# CRM 生产冒烟（radishcode 上线门调用）：登录 → kanban API 断言 → 前端 bundle 文案断言
# 凭据走环境变量：CRM_SMOKE_USER / CRM_SMOKE_PASSWORD（不落库、不进 git）
# 退出码：0=冒烟通过；非 0=失败（内核会把任务打回开发）
set -euo pipefail

BASE="${CRM_BASE:-https://bionicroot.cn/crm}"
USER="${CRM_SMOKE_USER:?need CRM_SMOKE_USER}"
PASS="${CRM_SMOKE_PASSWORD:?need CRM_SMOKE_PASSWORD}"

echo "[smoke] 登录 $BASE ..."
TOKEN=$(curl -fsS -X POST "$BASE/api/auth/login" \
  -H "Content-Type: application/json" \
  -d "{\"username\":\"$USER\",\"password\":\"$PASS\"}" \
  | python -c "import sys,json;print(json.load(sys.stdin)['token'])")
echo "[smoke] 登录成功"

echo "[smoke] 看板 API 断言 ..."
curl -fsS "$BASE/api/pipeline/kanban" -H "Authorization: Bearer $TOKEN" | python -c "
import sys, json
data = json.load(sys.stdin)
assert isinstance(data, list) and len(data) > 0, 'kanban: 无阶段列'
cards = [c for col in data for c in col.get('customers', [])]
print(f'[smoke] 看板 {len(data)} 列 / {len(cards)} 卡片')
if cards:
    assert 'owner_name' in cards[0], 'kanban: 卡片缺 owner_name 字段'
    print('[smoke] owner_name 字段在场')
"

echo "[smoke] 前端 bundle 文案断言 ..."
JS_PATH=$(curl -fsS "$BASE/" | grep -o 'src="[^"]*\.js"' | head -1 | sed 's/src="//;s/"//')
[ -n "$JS_PATH" ] || { echo "[smoke] 找不到前端 bundle"; exit 1; }
# JS_PATH 形如 /crm/assets/index-xxx.js，拼站点根
curl -fsS "https://bionicroot.cn${JS_PATH}" | grep -q "加入手头" || { echo "[smoke] bundle 缺看板交互文案"; exit 1; }
echo "[smoke] bundle 含看板交互（加入手头）"

echo "SMOKE PASS"
