import { useEffect, useState } from 'react'
import axios from 'axios'
import {
  Button, Callout, Card, Checkbox, EmptyState, ErrorState, Field, Input, LoadingState,
  PageHeader,
} from '../components/ui'

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
  const [entLoading, setEntLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const [entError, setEntError] = useState('')
  const [msg, setMsg] = useState('')
  const [msgTone, setMsgTone] = useState('success')

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
    setEntError('')
    setList([])
    setOriginal([])
    setEntLoading(true)
    try {
      const resp = await axios.get(`${baseURL}/admin/tenants/${t.id}/entitlements`, ownerClient(token))
      setList(resp.data)
      setOriginal(resp.data)
    } catch (e) {
      setEntError(e?.response?.data?.error || '加载授权清单失败')
    } finally {
      setEntLoading(false)
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
      setMsgTone('info')
      setMsg('没有变更')
      return
    }
    setSaving(true)
    try {
      const resp = await axios.put(
        `${baseURL}/admin/tenants/${selected.id}/entitlements`,
        changes,
        ownerClient(token),
      )
      setList(resp.data)
      setOriginal(resp.data)
      setMsgTone('success')
      setMsg('已保存，变更已记入审计日志')
    } catch (e) {
      setError(e?.response?.data?.error || '保存失败')
    } finally {
      setSaving(false)
    }
  }

  if (!authed) {
    return (
      <div className="owner-auth">
        <Card title="工作室管理终端" subtitle="输入 owner 令牌管理各租户功能包">
          {error && <Callout tone="danger">{error}</Callout>}
          <Field label="owner 令牌" htmlFor="owner-token">
            <Input
              id="owner-token"
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && submitToken(token)}
            />
          </Field>
          <Button
            variant="primary"
            block
            className="u-mt-3"
            loading={loading}
            disabled={!token}
            onClick={() => submitToken(token)}
          >
            {loading ? '验证中…' : '进入'}
          </Button>
        </Card>
      </div>
    )
  }

  return (
    <div className="owner-page">
      <PageHeader
        title="租户功能包管理"
        description="按租户开关功能项；仅提交有变化的键，变更记入审计日志"
      />
      {error && <Callout tone="danger">{error}</Callout>}

      <div className="owner-layout">
        <div className="owner-side">
          <Card title="租户列表" subtitle={`${tenants.length} 个租户`}>
            {tenants.length === 0 ? (
              <EmptyState
                inline
                title="暂无租户"
                description="后端尚未创建任何租户；先创建租户后才能配置功能包。"
              />
            ) : (
              <div className="tenant-list">
                {tenants.map((t) => (
                  <div
                    key={t.id}
                    className={`tenant-item${selected?.id === t.id ? ' is-active' : ''}`}
                    onClick={() => openTenant(t)}
                  >
                    <div className="tenant-name">{t.name}</div>
                    <div className="tenant-meta">{t.code} · {t.status}</div>
                  </div>
                ))}
              </div>
            )}
          </Card>
        </div>

        <div className="owner-main">
          <Card title={selected ? `功能包：${selected.name}` : '功能包'}>
            {!selected ? (
              <EmptyState
                inline
                title="从左侧选择租户"
                description="选择租户后可查看并调整该租户的功能包开关。"
              />
            ) : entLoading ? (
              <LoadingState label="加载授权清单…" />
            ) : entError ? (
              <ErrorState
                title="授权清单加载失败"
                message={entError}
                onRetry={() => openTenant(selected)}
              />
            ) : list.length === 0 ? (
              <EmptyState
                inline
                title="暂无功能项"
                description="该租户当前没有可配置的功能项。"
              />
            ) : (
              <>
                {msg && <Callout tone={msgTone}>{msg}</Callout>}
                {[1, 2, 3].map((tier) => {
                  const tierList = list.filter((e) => e.tier === tier)
                  if (tierList.length === 0) return null
                  return (
                    <div key={tier} className="tier-group">
                      <div className="tier-title">{TIER_NAMES[tier]}</div>
                      {tierList.map((e) => (
                        <div key={e.key} className="entitlement-row">
                          <Checkbox
                            checked={!!e.enabled}
                            onChange={() => toggle(e.key)}
                            label={e.label}
                          />
                          <span className="entitlement-key">
                            {e.key}{e.source === 'default' ? '（默认档）' : ''}
                          </span>
                        </div>
                      ))}
                    </div>
                  )
                })}
                <Button variant="primary" className="u-mt-4" onClick={save} loading={saving}>
                  保存
                </Button>
              </>
            )}
          </Card>
        </div>
      </div>
    </div>
  )
}
