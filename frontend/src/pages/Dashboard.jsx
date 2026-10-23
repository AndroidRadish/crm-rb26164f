import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import {
  Button, Callout, Card, CardSkeleton, EmptyState, ErrorState, Field, Input,
  PageHeader, Skeleton, StatCard,
} from '../components/ui'
import { getErrorMessage } from '../api/client'
import FeatureGate from '../components/FeatureGate'
import { useAuth } from '../auth'

export default function Dashboard() {
  const { user, isAdmin } = useAuth()
  const navigate = useNavigate()
  const [stats, setStats] = useState(null)
  const [funnel, setFunnel] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  const load = async () => {
    setLoading(true)
    try {
      const [s, f] = await Promise.all([api.getDashboard(), api.getFunnel()])
      setStats(s)
      setFunnel(f)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载数据看板失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  const maxCount = Math.max(...funnel.map((f) => f.count), 1)
  const isEmpty = Boolean(stats) && stats.total_customers === 0 && funnel.length === 0

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <PageHeader
          title="数据看板"
          description="团队客户与成交概况"
          actions={<Button variant="secondary" onClick={load} disabled={loading}>刷新</Button>}
        />

        {loading ? (
          <CardSkeleton count={4} />
        ) : error && !stats ? (
          <ErrorState message={error} onRetry={load} />
        ) : (
          <>
            {error && (
              <Callout tone="danger" actions={<Button size="sm" variant="secondary" onClick={load}>重试</Button>}>
                {error}
              </Callout>
            )}

            {isEmpty ? (
              <Card>
                <EmptyState
                  title="还没有客户数据"
                  description="新增客户并记录跟进后，这里会展示客户总量、赢单率与销售漏斗。"
                  action={<Button variant="primary" onClick={() => navigate('/customers')}>去客户库新增</Button>}
                />
              </Card>
            ) : (
              <>
                {stats && (
                  <div className="stat-grid">
                    <StatCard label="客户总数" value={stats.total_customers} tone="accent" />
                    <StatCard label="跟进次数" value={stats.total_activities} />
                    <StatCard label="赢单数" value={stats.won_count} />
                    <StatCard label="赢单率" value={`${stats.win_rate.toFixed(1)}%`} />
                  </div>
                )}

                <Card title="销售漏斗" subtitle="各阶段客户分布">
                  {funnel.length === 0 ? (
                    <EmptyState
                      inline
                      title="暂无漏斗数据"
                      description="客户还未进入任何销售阶段；在客户详情里设置阶段后即可看到分布。"
                    />
                  ) : (
                    <div className="funnel">
                      {funnel.map((f) => (
                        <div key={f.stage_id} className="funnel-row">
                          <div className="funnel-label" title={f.name}>{f.name}</div>
                          <div className="funnel-track">
                            <div
                              className="funnel-bar"
                              style={{ width: `${(f.count / maxCount) * 100}%` }}
                            >
                              {f.count}
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </Card>

                {stats && stats.pending_tasks > 0 && (
                  <div className="u-mt-4">
                    <Callout
                      tone="info"
                      title="待办提醒"
                      actions={<Button size="sm" variant="secondary" onClick={() => navigate('/tasks')}>查看待办</Button>}
                    >
                      你有 <strong>{stats.pending_tasks}</strong> 个待办任务待处理。
                    </Callout>
                  </div>
                )}
              </>
            )}

            {/* 写仍 admin（后端 PUT /settings 仅 admin）：member 端读到的 webhook 是
                「已配置/未配置」布尔而非明文，这里也不再给 member 渲染不可写的表单 */}
            {isAdmin && (
              <FeatureGate feature="notify.webhook">
                <WebhookSettings />
              </FeatureGate>
            )}
          </>
        )}
      </Page>
    </div>
  )
}

function WebhookSettings() {
  const [form, setForm] = useState({ wecom_webhook_url: '', dingtalk_webhook_url: '' })
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [msg, setMsg] = useState('')
  const [msgTone, setMsgTone] = useState('success')

  useEffect(() => {
    let alive = true
    api.getSettings()
      .then((d) => { if (alive) setForm((f) => ({ ...f, ...d })) })
      .catch((e) => {
        if (!alive) return
        setMsgTone('danger')
        setMsg(getErrorMessage(e, '读取提醒设置失败'))
      })
      .finally(() => { if (alive) setLoading(false) })
    return () => { alive = false }
  }, [])

  const save = async () => {
    setMsg('')
    setSaving(true)
    try {
      await api.putSettings(form)
      setMsgTone('success')
      setMsg('已保存，企微/钉钉提醒已生效')
    } catch (e) {
      setMsgTone('danger')
      setMsg(getErrorMessage(e, '保存失败'))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Card title="提醒设置" subtitle="企业微信 / 钉钉机器人 webhook" className="u-mt-4">
      {loading ? (
        <div className="u-col u-gap-3">
          <Skeleton className="skeleton-line" />
          <Skeleton className="skeleton-line" />
        </div>
      ) : (
        <>
          {msg && <Callout tone={msgTone}>{msg}</Callout>}
          <Field label="企业微信机器人 webhook" htmlFor="wecom-webhook">
            <Input
              id="wecom-webhook"
              value={form.wecom_webhook_url}
              onChange={(e) => setForm({ ...form, wecom_webhook_url: e.target.value })}
              placeholder="https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=..."
            />
          </Field>
          <Field label="钉钉机器人 webhook" htmlFor="dingtalk-webhook">
            <Input
              id="dingtalk-webhook"
              value={form.dingtalk_webhook_url}
              onChange={(e) => setForm({ ...form, dingtalk_webhook_url: e.target.value })}
              placeholder="https://oapi.dingtalk.com/robot/send?access_token=..."
            />
          </Field>
          <Button variant="primary" onClick={save} loading={saving}>保存</Button>
        </>
      )}
    </Card>
  )
}
