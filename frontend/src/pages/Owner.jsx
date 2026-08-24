import { useEffect, useState } from 'react'
import axios from 'axios'

// 工作室管理终端（owner）：独立于 CRM 租户登录，用 OWNER_TOKEN 令牌访问。
// 与租户用户体系正交，不走 auth.jsx / api client（避免 crm_token 覆盖 Authorization）。
const baseURL = import.meta.env.VITE_API_BASE || '/api'

const TIER_NAMES = { 1: '基础档', 2: '专业档', 3: '旗舰档' }

export default function Owner() {
  const [token, setToken] = useState(() => localStorage.getItem('crm_owner_token') || '')
  const [authed, setAuthed] = useState(false)
  const [tenants, setTenants] = useState([])
  const [selected, setSelected] = useState(null) // 当前配置的租户
  const [list, setList] = useState([]) // 授权清单（编辑中）
  const [original, setOriginal] = useState([]) // 上次保存时的清单（diff 用）
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [msg, setMsg] = useState('')

  const ownerClient = (t) => ({ headers: { Authorization: `Bearer ${t}` } })

  const loadTenants = async (t) => {
    const resp = await axios.get(`${baseURL}/admin/tenants`, ownerClient(t))
    setTenants(resp.data)
  }

  const submitToken = async (t) => {
    setError('')
    setLoading(true)
    try {
      await loadTenants(t)
      localStorage.setItem('crm_owner_token', t)
      setAuthed(true)
    } catch (e) {
      setError(e?.response?.data?.error || '令牌无效或后端未配置 OWNER_TOKEN')
      setAuthed(false)
    } finally {
      setLoading(false)
    }
  }

  // 已存令牌自动登录
  useEffect(() => {
    if (token) submitToken(token)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const openTenant = async (t) => {
    setSelected(t)
    setMsg('')
    setError('')
    try {
      const resp = await axios.get(`${baseURL}/admin/tenants/${t.id}/entitlements`, ownerClient(token))
      setList(resp.data)
      setOriginal(resp.data)
    } catch (e) {
      setError(e?.response?.data?.error || '加载授权清单失败')
    }
  }

  const toggle = (key) => {
    setList(list.map((e) => (e.key === key ? { ...e, enabled: !e.enabled } : e)))
  }

  const save = async () => {
    setMsg('')
    setError('')
    // 只提交有变化的键，避免把默认档固化成显式记录
    const changes = {}
    for (const e of list) {
      const before = original.find((o) => o.key === e.key)
      if (before && before.enabled !== e.enabled) changes[e.key] = e.enabled
    }
    if (Object.keys(changes).length === 0) {
      setMsg('没有变更')
      return
    }
    try {
      const resp = await axios.put(
        `${baseURL}/admin/tenants/${selected.id}/entitlements`,
        changes,
        ownerClient(token),
      )
      setList(resp.data)
      setOriginal(resp.data)
      setMsg('已保存，变更已记入审计日志')
    } catch (e) {
      setError(e?.response?.data?.error || '保存失败')
    }
  }

  if (!authed) {
    return (
      <div style={{ maxWidth: 420, margin: '80px auto', padding: '0 16px' }}>
        <div className="card">
          <div className="card-title">工作室管理终端</div>
          <p style={{ fontSize: 13, margin: '8px 0 16px' }}>输入 owner 令牌管理各租户功能包</p>
          {error && <div className="form-error">{error}</div>}
          <div className="form-group">
            <label className="form-label">owner 令牌</label>
            <input
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && submitToken(token)}
            />
          </div>
          <button className="btn btn-primary" disabled={loading || !token} onClick={() => submitToken(token)}>
            {loading ? '验证中…' : '进入'}
          </button>
        </div>
      </div>
    )
  }

  return (
    <div style={{ maxWidth: 960, margin: '32px auto', padding: '0 16px' }}>
      <h1 className="page-title">租户功能包管理</h1>
      {error && <div className="form-error">{error}</div>}
      <div style={{ display: 'flex', gap: 24, alignItems: 'flex-start', flexWrap: 'wrap' }}>
        <div className="card" style={{ minWidth: 280, flex: '0 0 320px' }}>
          <div className="card-title">租户列表</div>
          {tenants.length === 0 ? (
            <div className="empty-state">暂无租户</div>
          ) : (
            tenants.map((t) => (
              <div
                key={t.id}
                onClick={() => openTenant(t)}
                style={{
                  padding: '8px 10px',
                  cursor: 'pointer',
                  borderRadius: 6,
                  background: selected?.id === t.id ? 'var(--color-accent-soft, #eef2ff)' : 'transparent',
                }}
              >
                <div style={{ fontWeight: 600 }}>{t.name}</div>
                <div style={{ fontSize: 12, color: '#888' }}>{t.code} · {t.status}</div>
              </div>
            ))
          )}
        </div>

        <div className="card" style={{ flex: 1, minWidth: 320 }}>
          <div className="card-title">
            {selected ? `功能包：${selected.name}` : '从左侧选择租户'}
          </div>
          {selected && (
            <>
              {[1, 2, 3].map((tier) => (
                <div key={tier} style={{ marginTop: 16 }}>
                  <div style={{ fontWeight: 600, marginBottom: 8 }}>{TIER_NAMES[tier]}</div>
                  {list
                    .filter((e) => e.tier === tier)
                    .map((e) => (
                      <label key={e.key} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 0' }}>
                        <input type="checkbox" checked={!!e.enabled} onChange={() => toggle(e.key)} />
                        <span>{e.label}</span>
                        <span style={{ fontSize: 12, color: '#888' }}>
                          {e.key}{e.source === 'default' ? '（默认档）' : ''}
                        </span>
                      </label>
                    ))}
                </div>
              ))}
              <button className="btn btn-primary" style={{ marginTop: 16 }} onClick={save}>
                保存
              </button>
              {msg && <div className="form-hint" style={{ marginTop: 8 }}>{msg}</div>}
            </>
          )}
        </div>
      </div>
    </div>
  )
}
